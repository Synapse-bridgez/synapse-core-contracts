//! Tests for fee accrual (#141) and treasury withdrawal (#142).
//!
//! ## Issue #141 — Fee accrual
//! - Verifies basis-points fee calculation at completion
//! - Verifies on-chain treasury balance is updated after each completion
//! - Verifies no fee is accrued for a 0 bps setting
//! - Verifies `fee` event is emitted on accrual
//! - Verifies the `base_fee_bps` registry param is range-checked and admin-gated
//! - Verifies fee rate change applies to future completions only
//!
//! ## Issue #142 — Treasury withdrawal
//! - Verifies `propose_withdrawal` is admin-gated
//! - Verifies `authorize_withdrawal` requires relay signer co-authorization
//! - Verifies per-epoch cap is enforced
//! - Verifies epoch resets after epoch_length ledgers
//! - Verifies withdrawal events are emitted
//! - Verifies withdrawal cannot exceed treasury balance
//! - Verifies a single party cannot both propose and authorize

#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, String, Symbol, TryFromVal,
};

use crate::types::{CallbackPayload, CallbackType, ContractError, TreasuryConfig};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Set the `base_fee_bps` registry param (admin auth must be mocked).
fn set_fee(env: &Env, client: &SynapseCoreContractClient, bps: i128) {
    client.set_param(&String::from_str(env, "base_fee_bps"), &bps);
}

/// Set the treasury epoch cap/length registry params (admin auth must be mocked).
fn set_treasury(env: &Env, client: &SynapseCoreContractClient, cap: i128, length: i128) {
    client.set_param(&String::from_str(env, "treasury_epoch_cap"), &cap);
    client.set_param(&String::from_str(env, "treasury_epoch_length"), &length);
}

/// Register + initialise the contract with all auths mocked.
/// Uses 100 bps (1%) fee and a 1_000_000 stroop epoch cap.
fn setup_with_fee(fee_bps: u32) -> (Env, SynapseCoreContractClient<'static>, Address, Address) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    set_fee(&env, &client, i128::from(fee_bps));
    set_treasury(&env, &client, 1_000_000, 1_000);
    (env, client, admin, relay)
}

/// A real SEP-23 ed25519 public-key strkey (checksum-valid).
fn g_address(env: &Env) -> String {
    String::from_str(
        env,
        "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ",
    )
}

fn make_payload(env: &Env, tx_id: &str, idem_key: &str, amount: i128) -> CallbackPayload {
    let account = g_address(env);
    CallbackPayload {
        transaction_id: String::from_str(env, tx_id),
        stellar_account: account.clone(),
        amount,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: account,
        idempotency_key: String::from_str(env, idem_key),
        anchor_transaction_id: String::from_str(env, "anchor-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    }
}

/// Register, start processing, and complete a transaction. Returns the tx_id.
fn complete_tx(
    env: &Env,
    client: &SynapseCoreContractClient,
    relay: &Address,
    tx_id: &str,
    idem_key: &str,
    amount: i128,
) -> String {
    let payload = make_payload(env, tx_id, idem_key, amount);
    let id = client.register_callback(&payload);
    client.start_processing(&id, relay);
    client.complete_transaction(&id, &String::from_str(env, "hash-x"), relay);
    id
}

/// Assert the last two topics of an event are `synapse` / `name`.
fn assert_topics(
    env: &Env,
    topics: &soroban_sdk::Vec<soroban_sdk::Val>,
    name: soroban_sdk::Symbol,
) {
    assert_eq!(topics.len(), 2);
    let t0 = Symbol::try_from_val(env, &topics.get_unchecked(0)).unwrap();
    let t1 = Symbol::try_from_val(env, &topics.get_unchecked(1)).unwrap();
    assert_eq!(t0, symbol_short!("synapse"));
    assert_eq!(t1, name);
}

// ─── Issue #141: Fee accrual ──────────────────────────────────────────────────

#[test]
fn test_fee_accrual_zero_bps_accrues_nothing() {
    // With fee_bps=0, completing a transaction leaves the treasury at 0.
    let (env, client, _admin, relay) = setup_with_fee(0);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000);
    assert_eq!(client.treasury_balance(), 0);
}

#[test]
fn test_fee_accrual_100_bps_on_1_million() {
    // 100 bps = 1%; 1_000_000 stroops * 1% = 10_000 stroops.
    let (env, client, _admin, relay) = setup_with_fee(100);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000);
    assert_eq!(client.treasury_balance(), 10_000);
}

#[test]
fn test_fee_accrual_50_bps_on_2_million() {
    // 50 bps = 0.5%; 2_000_000 stroops * 0.5% = 10_000 stroops.
    let (env, client, _admin, relay) = setup_with_fee(50);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 2_000_000);
    assert_eq!(client.treasury_balance(), 10_000);
}

#[test]
fn test_fee_accrual_1_bps_minimum_rounds_down() {
    // 1 bps = 0.01%; 99 stroops * 0.01% < 1 stroop → rounds to 0 (no accrual).
    let (env, client, _admin, relay) = setup_with_fee(1);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 99);
    assert_eq!(client.treasury_balance(), 0);

    // 10_000 stroops * 1 bps = 1 stroop → rounds to exactly 1.
    complete_tx(&env, &client, &relay, "tx-2", "idem-2", 10_000);
    assert_eq!(client.treasury_balance(), 1);
}

#[test]
fn test_fee_accrual_max_bps() {
    // 10_000 bps = 100%; entire amount accrues.
    let (env, client, _admin, relay) = setup_with_fee(10_000);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 5_000);
    assert_eq!(client.treasury_balance(), 5_000);
}

#[test]
fn test_fee_accrual_is_cumulative() {
    // Multiple completions accumulate in the treasury.
    let (env, client, _admin, relay) = setup_with_fee(100); // 1%
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000); // 10_000 fee
    complete_tx(&env, &client, &relay, "tx-2", "idem-2", 2_000_000); // 20_000 fee
    assert_eq!(client.treasury_balance(), 30_000);
}

#[test]
fn test_fee_accrual_emits_fee_event() {
    // A fee event is emitted whenever a non-zero fee accrues.
    let (env, client, _admin, relay) = setup_with_fee(100);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000);

    let events = env.events().all();
    // complete_transaction emits: status, done, fee (in that order).
    assert_eq!(events.len(), 3);
    assert_topics(&env, &events.get_unchecked(0).1, symbol_short!("status"));
    assert_topics(&env, &events.get_unchecked(1).1, symbol_short!("done"));
    assert_topics(&env, &events.get_unchecked(2).1, symbol_short!("fee"));
}

#[test]
fn test_fee_accrual_zero_bps_no_fee_event() {
    // With fee_bps=0, no fee event is emitted.
    let (env, client, _admin, relay) = setup_with_fee(0);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000);

    let events = env.events().all();
    // Only status and done events — no fee event.
    assert_eq!(events.len(), 2);
    assert_topics(&env, &events.get_unchecked(0).1, symbol_short!("status"));
    assert_topics(&env, &events.get_unchecked(1).1, symbol_short!("done"));
}

#[test]
fn test_fee_not_accrued_for_failed_transaction() {
    // Failing a transaction must NOT accrue a fee — fees are only on Completed.
    let (env, client, _admin, relay) = setup_with_fee(100);
    let payload = make_payload(&env, "tx-1", "idem-1", 1_000_000);
    let tx_id = client.register_callback(&payload);
    client.start_processing(&tx_id, &relay);
    client.fail_transaction(&tx_id, &String::from_str(&env, "timeout"), &relay);
    assert_eq!(client.treasury_balance(), 0);
}

fn param(env: &Env, name: &str) -> String {
    String::from_str(env, name)
}

#[test]
fn test_base_fee_bps_param_accepts_bounds() {
    let (env, client, _admin, _relay) = setup_with_fee(0);
    for bps in [0_i128, 1, 10_000] {
        set_fee(&env, &client, bps);
        assert_eq!(client.get_param(&param(&env, "base_fee_bps")).value, bps);
    }
}

#[test]
fn test_base_fee_bps_param_rejects_out_of_range() {
    let (env, client, _admin, _relay) = setup_with_fee(100);
    for bad in [10_001_i128, -1, i128::MAX] {
        assert_eq!(
            client.try_set_param(&param(&env, "base_fee_bps"), &bad),
            Err(Ok(ContractError::InvalidParamValue))
        );
    }
    // Rejected writes leave the previous rate in place.
    assert_eq!(client.get_param(&param(&env, "base_fee_bps")).value, 100);
}

#[test]
fn test_fee_rate_param_rejects_non_admin() {
    // Only the admin may change the fee rate.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    client.initialize(&admin, &relay);

    let attacker = Address::generate(&env);
    let name = param(&env, "base_fee_bps");
    let result = client
        .mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "set_param",
                args: (name.clone(), 200_i128).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_set_param(&name, &200);
    assert!(result.is_err());
    assert_eq!(
        client.try_get_param(&name),
        Err(Ok(ContractError::ParamNotFound))
    );
}

#[test]
fn test_fee_rate_change_emits_param_event() {
    let (env, client, _admin, _relay) = setup_with_fee(50);
    set_fee(&env, &client, 200);

    let events = env.events().all();
    let last = events.get_unchecked(events.len() - 1);
    assert_topics(&env, &last.1, symbol_short!("param"));
}

#[test]
fn test_unset_fee_param_accrues_nothing() {
    // No `base_fee_bps` param at all behaves as a zero rate.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000);
    assert_eq!(client.treasury_balance(), 0);
}

#[test]
fn test_fee_rate_change_only_affects_future_completions() {
    // Completions before the rate change use the old rate;
    // completions after use the new rate.
    let (env, client, _admin, relay) = setup_with_fee(100); // 1%
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000); // 10_000 fee
    assert_eq!(client.treasury_balance(), 10_000);

    set_fee(&env, &client, 200); // 2%
    complete_tx(&env, &client, &relay, "tx-2", "idem-2", 1_000_000); // 20_000 fee
    assert_eq!(client.treasury_balance(), 30_000);
}

// ─── Issue #142: Treasury withdrawal ─────────────────────────────────────────

#[test]
fn test_propose_withdrawal_happy_path() {
    let (env, client, _admin, relay) = setup_with_fee(100);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000); // 10_000 fee
    let dest = Address::generate(&env);
    client.propose_withdrawal(&5_000, &dest);

    let pending = client
        .pending_withdrawal()
        .expect("should have pending withdrawal");
    assert_eq!(pending.amount, 5_000);
    assert_eq!(pending.destination, dest);
}

#[test]
fn test_propose_withdrawal_rejects_non_admin() {
    // Treasury is configured and funded, so the only thing that can reject
    // the attacker's proposal is the admin-auth check.
    let (env, client, _admin, relay) = setup_with_fee(100);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000); // 10_000 fee
    let contract_id = client.address.clone();

    let attacker = Address::generate(&env);
    let dest = Address::generate(&env);
    let result = client
        .mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "propose_withdrawal",
                args: (5_000_i128, dest.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_propose_withdrawal(&5_000, &dest);
    assert!(result.is_err());
    assert!(client.pending_withdrawal().is_none());
}

#[test]
fn test_propose_withdrawal_rejects_zero_amount() {
    let (env, client, _admin, relay) = setup_with_fee(100);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000);
    let dest = Address::generate(&env);
    let result = client.try_propose_withdrawal(&0, &dest);
    assert_eq!(result, Err(Ok(ContractError::InvalidWithdrawalAmount)));
}

#[test]
fn test_propose_withdrawal_rejects_exceeds_balance() {
    let (env, client, _admin, relay) = setup_with_fee(100);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000); // 10_000 fee
    let dest = Address::generate(&env);
    let result = client.try_propose_withdrawal(&10_001, &dest);
    assert_eq!(result, Err(Ok(ContractError::InsufficientTreasuryBalance)));
}

#[test]
fn test_propose_withdrawal_rejects_when_treasury_not_configured() {
    // If no TreasuryConfig was provided at init, proposals must be rejected.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    set_fee(&env, &client, 100);
    // Directly set treasury balance to bypass the fee-accrual path
    // (treasury config is required first, but this tests the proposal check).
    let dest = Address::generate(&env);
    let result = client.try_propose_withdrawal(&1, &dest);
    assert_eq!(result, Err(Ok(ContractError::TreasuryNotConfigured)));
}

#[test]
fn test_authorize_withdrawal_happy_path() {
    let (env, client, _admin, relay) = setup_with_fee(100);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000); // 10_000 fee
    let dest = Address::generate(&env);

    client.propose_withdrawal(&5_000, &dest);
    assert_eq!(client.treasury_balance(), 10_000);

    client.authorize_withdrawal(&relay, &5_000, &dest);
    assert_eq!(client.treasury_balance(), 5_000);
    assert!(client.pending_withdrawal().is_none());
}

#[test]
fn test_authorize_withdrawal_requires_relay_not_admin() {
    // The relay signer must co-authorize; the admin cannot unilaterally execute.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    set_fee(&env, &client, 100);
    set_treasury(&env, &client, 1_000_000, 1_000);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000);
    let dest = Address::generate(&env);
    client.propose_withdrawal(&5_000, &dest);

    // Admin tries to authorize — not allowed; only the relay signer may.
    let result = client.try_authorize_withdrawal(&admin, &5_000, &dest);
    assert_eq!(result, Err(Ok(ContractError::WithdrawalNotRelaySigner)));

    // Treasury is unaffected.
    assert_eq!(client.treasury_balance(), 10_000);
    // Proposal still pending.
    assert!(client.pending_withdrawal().is_some());
}

#[test]
fn test_authorize_withdrawal_requires_relay_signer_auth() {
    // Passing caller=relay but authorizing with a different address must fail.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    set_fee(&env, &client, 100);
    set_treasury(&env, &client, 1_000_000, 1_000);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000);
    let dest = Address::generate(&env);
    client.propose_withdrawal(&5_000, &dest);

    // Pass caller=relay but supply a different account's auth.
    let attacker = Address::generate(&env);
    let result = client
        .mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "authorize_withdrawal",
                args: (relay.clone(), 5_000_i128, dest.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_authorize_withdrawal(&relay, &5_000, &dest);
    assert!(result.is_err());
}

#[test]
fn test_authorize_withdrawal_rejects_no_pending() {
    let (_env, client, _admin, relay) = setup_with_fee(100);
    let result = client.try_authorize_withdrawal(&relay, &1, &relay);
    assert_eq!(result, Err(Ok(ContractError::NoPendingWithdrawal)));
}

#[test]
fn test_authorize_withdrawal_epoch_cap_enforced() {
    // Cap = 1_000_000 stroops per epoch.
    // Build up enough treasury first.
    let (env, client, _admin, relay) = setup_with_fee(10_000); // 100% fee rate
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 3_000_000);
    assert_eq!(client.treasury_balance(), 3_000_000);

    let dest = Address::generate(&env);

    // First withdrawal of 1_000_000 — exactly at the cap.
    client.propose_withdrawal(&1_000_000, &dest);
    client.authorize_withdrawal(&relay, &1_000_000, &dest);
    assert_eq!(client.treasury_balance(), 2_000_000);

    // Second withdrawal in the same epoch would exceed the cap.
    client.propose_withdrawal(&1, &dest);
    let result = client.try_authorize_withdrawal(&relay, &1, &dest);
    assert_eq!(result, Err(Ok(ContractError::WithdrawalCapExceeded)));
}

#[test]
fn test_authorize_withdrawal_epoch_resets_after_epoch_length() {
    // After epoch_length ledgers, the cap resets and a new withdrawal is allowed.
    let (env, client, _admin, relay) = setup_with_fee(10_000); // 100% fee
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 3_000_000);

    let dest = Address::generate(&env);

    // Exhaust the cap in epoch 0.
    client.propose_withdrawal(&1_000_000, &dest);
    client.authorize_withdrawal(&relay, &1_000_000, &dest);

    // Try immediately — still in epoch 0, cap exhausted.
    client.propose_withdrawal(&1, &dest);
    assert_eq!(
        client.try_authorize_withdrawal(&relay, &1, &dest),
        Err(Ok(ContractError::WithdrawalCapExceeded))
    );

    // Advance past the epoch_length (1_000 ledgers).
    env.ledger().with_mut(|li| li.sequence_number += 1_001);

    // Now the epoch has reset; 1 stroop withdrawal should succeed.
    client.propose_withdrawal(&1, &dest);
    client.authorize_withdrawal(&relay, &1, &dest);
    assert_eq!(client.treasury_balance(), 1_999_999);
}

#[test]
fn test_authorize_withdrawal_emits_executed_event() {
    let (env, client, _admin, relay) = setup_with_fee(100);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000);
    let dest = Address::generate(&env);
    client.propose_withdrawal(&5_000, &dest);
    client.authorize_withdrawal(&relay, &5_000, &dest);

    let events = env.events().all();
    let last = events.get_unchecked(events.len() - 1);
    assert_topics(&env, &last.1, symbol_short!("wexec"));
}

#[test]
fn test_propose_withdrawal_emits_proposed_event() {
    let (env, client, _admin, relay) = setup_with_fee(100);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000);
    let dest = Address::generate(&env);
    client.propose_withdrawal(&5_000, &dest);

    let events = env.events().all();
    let last = events.get_unchecked(events.len() - 1);
    assert_topics(&env, &last.1, symbol_short!("wprop"));
}

#[test]
fn test_two_party_authorization_required() {
    // Key invariant: the admin proposes, the relay authorizes — a single
    // party cannot both propose and execute a withdrawal. This test verifies
    // that the relay signer is required for the second step (not the admin).
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    set_fee(&env, &client, 100);
    set_treasury(&env, &client, 1_000_000, 1_000);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000); // 10_000 fee

    let dest = Address::generate(&env);
    client.propose_withdrawal(&5_000, &dest);

    // Admin cannot finalize its own proposal.
    let result = client.try_authorize_withdrawal(&admin, &5_000, &dest);
    assert_eq!(result, Err(Ok(ContractError::WithdrawalNotRelaySigner)));

    // Treasury balance unchanged.
    assert_eq!(client.treasury_balance(), 10_000);
    assert!(client.pending_withdrawal().is_some());

    // Only the relay signer can co-authorize.
    client.authorize_withdrawal(&relay, &5_000, &dest);
    assert_eq!(client.treasury_balance(), 5_000);
}

#[test]
fn test_treasury_config_reflects_params() {
    let (env, client, _admin, _relay) = setup_with_fee(0);
    set_treasury(&env, &client, 500_000, 2_000);
    assert_eq!(
        client.treasury_config(),
        Some(TreasuryConfig {
            epoch_cap: 500_000,
            epoch_length: 2_000,
        })
    );
}

#[test]
fn test_treasury_config_none_until_both_params_set() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    assert_eq!(client.treasury_config(), None);
    client.set_param(&param(&env, "treasury_epoch_cap"), &1_000);
    assert_eq!(client.treasury_config(), None);
    client.set_param(&param(&env, "treasury_epoch_length"), &100);
    assert!(client.treasury_config().is_some());
}

#[test]
fn test_treasury_balance_query_reflects_accruals() {
    let (env, client, _admin, relay) = setup_with_fee(200); // 2%
    assert_eq!(client.treasury_balance(), 0);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000); // 20_000 fee
    assert_eq!(client.treasury_balance(), 20_000);
}

// ─── Checked arithmetic / numeric edges (#141, #142) ─────────────────────────

#[test]
fn test_compute_fee_matches_floor_formula_across_amounts() {
    // Cross-check the overflow-free split formula against the naive
    // `amount * bps / 10_000` wherever the naive product fits in i128.
    let amounts: [i128; 12] = [
        1,
        9_999,
        10_000,
        10_001,
        12_345,
        99_999_999,
        1_000_000_007,
        i64::MAX as i128,
        u64::MAX as i128,
        10_i128.pow(30) + 7,
        i128::MAX / 10_000,
        i128::MAX / 10_000 - 1,
    ];
    for bps in [0_u32, 1, 7, 30, 100, 2_500, 9_999, 10_000] {
        for amount in amounts {
            let naive = amount * i128::from(bps) / 10_000;
            assert_eq!(
                SynapseCoreContract::compute_fee(amount, bps),
                Ok(naive),
                "amount={amount} bps={bps}"
            );
        }
    }
}

#[test]
fn test_compute_fee_exact_at_i128_max() {
    // `i128::MAX * bps` would overflow; the split formula must not.
    assert_eq!(
        SynapseCoreContract::compute_fee(i128::MAX, 10_000),
        Ok(i128::MAX)
    );
    assert_eq!(
        SynapseCoreContract::compute_fee(i128::MAX, 1),
        Ok(i128::MAX / 10_000)
    );
    assert_eq!(
        SynapseCoreContract::compute_fee(i128::MAX, 5_000),
        Ok(i128::MAX / 2)
    );
}

#[test]
fn test_compute_fee_non_positive_amount_is_zero() {
    assert_eq!(SynapseCoreContract::compute_fee(0, 10_000), Ok(0));
    assert_eq!(SynapseCoreContract::compute_fee(-1, 10_000), Ok(0));
}

#[test]
fn test_fee_accrual_near_i128_max_amount() {
    let (env, client, _admin, relay) = setup_with_fee(1);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", i128::MAX);
    assert_eq!(client.treasury_balance(), i128::MAX / 10_000);
}

#[test]
fn test_treasury_overflow_is_rejected_not_wrapped() {
    let (env, client, _admin, relay) = setup_with_fee(10_000);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", i128::MAX);
    assert_eq!(client.treasury_balance(), i128::MAX);

    let payload = make_payload(&env, "tx-2", "idem-2", 1);
    let id = client.register_callback(&payload);
    client.start_processing(&id, &relay);
    let result = client.try_complete_transaction(&id, &String::from_str(&env, "hash-x"), &relay);
    assert_eq!(result, Err(Ok(ContractError::ArithmeticOverflow)));
    // The failed call is rolled back: balance untouched, tx not completed.
    assert_eq!(client.treasury_balance(), i128::MAX);
}

#[test]
fn test_treasury_params_reject_out_of_range() {
    let (env, client, _admin, _relay) = setup_with_fee(0);
    let cases: [(&str, i128); 5] = [
        ("treasury_epoch_cap", 0),
        ("treasury_epoch_cap", -1),
        ("treasury_epoch_length", 0),
        ("treasury_epoch_length", -1),
        ("treasury_epoch_length", i128::from(u32::MAX) + 1),
    ];
    for (name, value) in cases {
        assert_eq!(
            client.try_set_param(&param(&env, name), &value),
            Err(Ok(ContractError::InvalidParamValue)),
            "{name} = {value}"
        );
    }
    client.set_param(&param(&env, "treasury_epoch_length"), &i128::from(u32::MAX));
}

#[test]
fn test_unrelated_params_are_not_range_checked() {
    // Validation is scoped to the params this contract reads.
    let (env, client, _admin, _relay) = setup_with_fee(0);
    client.set_param(&param(&env, "operator_note"), &-5);
}

#[test]
fn test_epoch_cap_boundary_is_exact() {
    // Epoch length 1_000: the epoch opened at ledger L still applies at
    // L + 999 and resets at exactly L + 1_000.
    let (env, client, _admin, relay) = setup_with_fee(10_000);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 3_000_000);
    let dest = Address::generate(&env);

    // Cap - 1, then exactly 1 more reaches the cap exactly.
    client.propose_withdrawal(&999_999, &dest);
    client.authorize_withdrawal(&relay, &999_999, &dest);
    client.propose_withdrawal(&1, &dest);
    client.authorize_withdrawal(&relay, &1, &dest);

    env.ledger().with_mut(|li| li.sequence_number += 999);
    client.propose_withdrawal(&1, &dest);
    assert_eq!(
        client.try_authorize_withdrawal(&relay, &1, &dest),
        Err(Ok(ContractError::WithdrawalCapExceeded))
    );

    env.ledger().with_mut(|li| li.sequence_number += 1);
    client.authorize_withdrawal(&relay, &1, &dest);
    assert_eq!(client.treasury_balance(), 1_999_999);
}

#[test]
fn test_balance_and_cap_errors_are_distinct() {
    // Balance 10_000 < cap 1_000_000: exceeding the balance is reported as
    // InsufficientTreasuryBalance, never as WithdrawalCapExceeded.
    let (env, client, _admin, relay) = setup_with_fee(100);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000);
    let dest = Address::generate(&env);
    assert_eq!(
        client.try_propose_withdrawal(&10_001, &dest),
        Err(Ok(ContractError::InsufficientTreasuryBalance))
    );

    // Configure a cap below the balance, then exceed only the cap.
    set_treasury(&env, &client, 5_000, 1_000);
    client.propose_withdrawal(&5_001, &dest);
    assert_eq!(
        client.try_authorize_withdrawal(&relay, &5_001, &dest),
        Err(Ok(ContractError::WithdrawalCapExceeded))
    );
}

#[test]
fn test_authorize_withdrawal_rejects_mismatched_amount_or_destination() {
    let (env, client, _admin, relay) = setup_with_fee(100);
    complete_tx(&env, &client, &relay, "tx-1", "idem-1", 1_000_000);
    let dest = Address::generate(&env);
    let other = Address::generate(&env);
    client.propose_withdrawal(&5_000, &dest);

    assert_eq!(
        client.try_authorize_withdrawal(&relay, &4_999, &dest),
        Err(Ok(ContractError::WithdrawalProposalMismatch))
    );
    assert_eq!(
        client.try_authorize_withdrawal(&relay, &5_000, &other),
        Err(Ok(ContractError::WithdrawalProposalMismatch))
    );
    assert_eq!(client.treasury_balance(), 10_000);
    assert!(client.pending_withdrawal().is_some());
}
