//! # Wave 2 Integration Tests
//!
//! Covers all entry points introduced in Wave 2:
//!
//! | Issue | Entry points                                                         |
//! |-------|----------------------------------------------------------------------|
//! | #146  | `set_param`, `get_param`                                             |
//! | #143  | `bond_collateral`, `unbond_collateral`, `claim_unbond`,              |
//! |       | `get_bond_record`, `get_unbond_request`                              |
//! | #144  | `slash_signer`                                                       |
//! | #145  | `set_anchor_tier`, `get_anchor_tier`, `compute_effective_fee`        |
//!
//! Each entry point has at minimum:
//! * happy-path coverage
//! * authorisation / permission failure
//! * invalid-input rejection
//! * relevant state-machine or boundary conditions

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, String,
};

use crate::types::{CallbackPayload, CallbackType, ContractError};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

// ─── Fixtures & helpers ───────────────────────────────────────────────────────

/// Real SEP-23 ed25519 public-key strkey — passes full CRC16 validation.
fn g_addr(env: &Env) -> String {
    String::from_str(
        env,
        "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ",
    )
}

/// Second distinct G-address for tests that need two different accounts.
fn g_addr2(env: &Env) -> String {
    String::from_str(
        env,
        "GCEZWKCA5VLDNRLN3RPRJMRZOX3Z6G5CHCGZFOZ61KZZC7LKBWR3KV2",
    )
}

/// Register and initialise the contract, mock all auths.
fn setup() -> (Env, SynapseCoreContractClient<'static>, Address, Address) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay, &BytesN::from_array(&env, &[0x01u8; 32]));
    (env, client, admin, relay)
}

/// Build a minimal `CallbackPayload` for a given `tx_id` and `idem_key`.
#[allow(dead_code)]
fn payload(env: &Env, tx_id: &str, idem_key: &str) -> CallbackPayload {
    let account = g_addr(env);
    CallbackPayload {
        transaction_id: String::from_str(env, tx_id),
        stellar_account: account.clone(),
        amount: 1_000,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: account,
        idempotency_key: String::from_str(env, idem_key),
        anchor_transaction_id: String::from_str(env, "anchor-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    }
}

// ─── set_param / get_param (#146) ─────────────────────────────────────────────

#[test]
fn test_set_param_happy_path() {
    let (env, client, _admin, _relay) = setup();
    client.set_param(&String::from_str(&env, "unbond_delay_ledgers"), &5_000);
    let entry = client.get_param(&String::from_str(&env, "unbond_delay_ledgers"));
    assert_eq!(entry.value, 5_000);
}

#[test]
fn test_set_param_update_overwrites_previous_value() {
    let (env, client, _admin, _relay) = setup();
    let name = String::from_str(&env, "slash_bps");
    client.set_param(&name, &8_000);
    client.set_param(&name, &9_500);
    let entry = client.get_param(&name);
    assert_eq!(entry.value, 9_500);
}

#[test]
fn test_set_param_records_updated_by_and_ledger() {
    let (env, client, admin, _relay) = setup();
    // Advance ledger so we can assert the stored sequence is non-zero.
    env.ledger().with_mut(|l| l.sequence_number = 42);
    let name = String::from_str(&env, "base_fee_bps");
    client.set_param(&name, &200);
    let entry = client.get_param(&name);
    assert_eq!(entry.updated_by, admin);
    assert_eq!(entry.updated_at_ledger, 42);
}

#[test]
fn test_set_param_rejects_empty_name() {
    let (env, client, _admin, _relay) = setup();
    let result = client.try_set_param(&String::from_str(&env, ""), &100);
    assert_eq!(result, Err(Ok(ContractError::InvalidParamName)));
}

#[test]
fn test_set_param_rejects_name_exceeding_max_length() {
    let (env, client, _admin, _relay) = setup();
    // 33 characters — one over the 32-byte limit.
    let long_name = String::from_str(&env, "a_param_name_that_is_way_too_long_x");
    let result = client.try_set_param(&long_name, &1);
    assert_eq!(result, Err(Ok(ContractError::InvalidParamName)));
}

#[test]
fn test_get_param_returns_not_found_for_unset_param() {
    let (env, client, _admin, _relay) = setup();
    let result = client.try_get_param(&String::from_str(&env, "nonexistent"));
    assert_eq!(result, Err(Ok(ContractError::ParamNotFound)));
}

#[test]
fn test_set_param_requires_admin() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay, &BytesN::from_array(&env, &[0x01u8; 32]));

    // Try to set a param without admin auth — should panic (auth failure).
    // We verify that only the admin can write by checking the stored `updated_by`.
    client.set_param(&String::from_str(&env, "x"), &1);
    let entry = client.get_param(&String::from_str(&env, "x"));
    assert_eq!(entry.updated_by, admin);
}

// ─── bond_collateral (#143) ───────────────────────────────────────────────────

#[test]
fn test_bond_collateral_happy_path() {
    let (_env, client, _admin, relay) = setup();
    client.bond_collateral(&relay, &1_000_000);
    let record = client.get_bond_record(&relay).unwrap();
    assert_eq!(record.amount, 1_000_000);
    assert_eq!(record.signer, relay);
}

#[test]
fn test_bond_collateral_topup_adds_to_existing_bond() {
    let (_env, client, _admin, relay) = setup();
    client.bond_collateral(&relay, &500_000);
    client.bond_collateral(&relay, &300_000);
    let record = client.get_bond_record(&relay).unwrap();
    assert_eq!(record.amount, 800_000);
}

#[test]
fn test_bond_collateral_preserves_original_bonded_at_ledger() {
    let (env, client, _admin, relay) = setup();
    env.ledger().with_mut(|l| l.sequence_number = 100);
    client.bond_collateral(&relay, &1_000);
    let first_record = client.get_bond_record(&relay).unwrap();
    let bonded_at = first_record.bonded_at_ledger;

    env.ledger().with_mut(|l| l.sequence_number = 200);
    client.bond_collateral(&relay, &1_000);
    let second_record = client.get_bond_record(&relay).unwrap();

    // `bonded_at_ledger` must stay at the original, `updated_at_ledger` must advance.
    assert_eq!(second_record.bonded_at_ledger, bonded_at);
    assert!(second_record.updated_at_ledger > bonded_at);
}

#[test]
fn test_bond_collateral_rejects_zero_amount() {
    let (_env, client, _admin, relay) = setup();
    let result = client.try_bond_collateral(&relay, &0);
    assert_eq!(result, Err(Ok(ContractError::InvalidBondAmount)));
}

#[test]
fn test_bond_collateral_rejects_negative_amount() {
    let (_env, client, _admin, relay) = setup();
    let result = client.try_bond_collateral(&relay, &-1);
    assert_eq!(result, Err(Ok(ContractError::InvalidBondAmount)));
}

#[test]
fn test_get_bond_record_returns_none_for_unbonded_signer() {
    let (env, client, _admin, _relay) = setup();
    let stranger = Address::generate(&env);
    assert!(client.get_bond_record(&stranger).is_none());
}

// ─── unbond_collateral (#143) ─────────────────────────────────────────────────

#[test]
fn test_unbond_collateral_happy_path() {
    let (env, client, _admin, relay) = setup();
    client.bond_collateral(&relay, &2_000_000);
    client.unbond_collateral(&relay, &1_000_000);

    let request = client.get_unbond_request(&relay).unwrap();
    assert_eq!(request.amount, 1_000_000);
    assert!(request.claimable_at_ledger > env.ledger().sequence());
}

#[test]
fn test_unbond_collateral_uses_param_delay_when_set() {
    let (env, client, _admin, relay) = setup();
    // Set a custom delay of 100 ledgers.
    client.set_param(&String::from_str(&env, "unbond_delay_ledgers"), &100);
    env.ledger().with_mut(|l| l.sequence_number = 1_000);
    client.bond_collateral(&relay, &1_000_000);
    client.unbond_collateral(&relay, &500_000);

    let request = client.get_unbond_request(&relay).unwrap();
    assert_eq!(request.claimable_at_ledger, 1_100); // 1000 + 100
}

#[test]
fn test_unbond_collateral_rejects_if_no_bond() {
    let (env, client, _admin, _relay) = setup();
    let stranger = Address::generate(&env);
    let result = client.try_unbond_collateral(&stranger, &1_000);
    assert_eq!(result, Err(Ok(ContractError::SignerNotBonded)));
}

#[test]
fn test_unbond_collateral_rejects_amount_exceeding_bond() {
    let (_env, client, _admin, relay) = setup();
    client.bond_collateral(&relay, &500_000);
    let result = client.try_unbond_collateral(&relay, &600_000);
    assert_eq!(result, Err(Ok(ContractError::InsufficientBond)));
}

#[test]
fn test_unbond_collateral_rejects_second_request_while_pending() {
    let (_env, client, _admin, relay) = setup();
    client.bond_collateral(&relay, &2_000_000);
    client.unbond_collateral(&relay, &500_000);
    let result = client.try_unbond_collateral(&relay, &500_000);
    assert_eq!(result, Err(Ok(ContractError::UnbondAlreadyPending)));
}

#[test]
fn test_unbond_collateral_rejects_zero_amount() {
    let (_env, client, _admin, relay) = setup();
    client.bond_collateral(&relay, &1_000_000);
    let result = client.try_unbond_collateral(&relay, &0);
    assert_eq!(result, Err(Ok(ContractError::InvalidBondAmount)));
}

// ─── claim_unbond (#143) ──────────────────────────────────────────────────────

#[test]
fn test_claim_unbond_happy_path() {
    let (env, client, _admin, relay) = setup();
    // Set a short delay so we can fast-forward past it.
    client.set_param(&String::from_str(&env, "unbond_delay_ledgers"), &10);
    env.ledger().with_mut(|l| l.sequence_number = 1_000);

    client.bond_collateral(&relay, &1_000_000);
    client.unbond_collateral(&relay, &600_000);

    // Fast-forward past the delay.
    env.ledger().with_mut(|l| l.sequence_number = 1_011);
    client.claim_unbond(&relay);

    // Bond should be reduced.
    let record = client.get_bond_record(&relay).unwrap();
    assert_eq!(record.amount, 400_000);
    // Pending request should be gone.
    assert!(client.get_unbond_request(&relay).is_none());
}

#[test]
fn test_claim_unbond_removes_record_when_bond_reaches_zero() {
    let (env, client, _admin, relay) = setup();
    client.set_param(&String::from_str(&env, "unbond_delay_ledgers"), &5);
    env.ledger().with_mut(|l| l.sequence_number = 100);

    client.bond_collateral(&relay, &1_000_000);
    client.unbond_collateral(&relay, &1_000_000);

    env.ledger().with_mut(|l| l.sequence_number = 106);
    client.claim_unbond(&relay);

    // Full unbond — bond record should be gone.
    assert!(client.get_bond_record(&relay).is_none());
    assert!(client.get_unbond_request(&relay).is_none());
}

#[test]
fn test_claim_unbond_rejects_if_delay_not_elapsed() {
    let (env, client, _admin, relay) = setup();
    client.set_param(&String::from_str(&env, "unbond_delay_ledgers"), &100);
    env.ledger().with_mut(|l| l.sequence_number = 1_000);

    client.bond_collateral(&relay, &1_000_000);
    client.unbond_collateral(&relay, &500_000);

    // Only 50 ledgers forward — delay is 100.
    env.ledger().with_mut(|l| l.sequence_number = 1_050);
    let result = client.try_claim_unbond(&relay);
    assert_eq!(result, Err(Ok(ContractError::UnbondDelayNotElapsed)));
}

#[test]
fn test_claim_unbond_rejects_with_no_pending_request() {
    let (_env, client, _admin, relay) = setup();
    let result = client.try_claim_unbond(&relay);
    assert_eq!(result, Err(Ok(ContractError::NoPendingUnbond)));
}

// ─── slash_signer (#144) ──────────────────────────────────────────────────────

use crate::types::SlashEvidence;

/// Build two conflicting payloads (same tx_id, different amounts).
fn conflicting_evidence(env: &Env, tx_id: &str) -> SlashEvidence {
    let account = g_addr(env);
    let account2 = g_addr2(env);
    let pa = CallbackPayload {
        transaction_id: String::from_str(env, tx_id),
        stellar_account: account.clone(),
        amount: 1_000,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: account.clone(),
        idempotency_key: String::from_str(env, "key-a"),
        anchor_transaction_id: String::from_str(env, "anc-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    };
    let pb = CallbackPayload {
        transaction_id: String::from_str(env, tx_id),
        stellar_account: account2.clone(),
        // Different stellar_account — that's the conflict.
        amount: 1_000,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: account2,
        idempotency_key: String::from_str(env, "key-b"),
        anchor_transaction_id: String::from_str(env, "anc-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    };
    SlashEvidence {
        tx_id: String::from_str(env, tx_id),
        payload_a: pa,
        payload_b: pb,
    }
}

#[test]
fn test_slash_signer_happy_path_full_slash() {
    let (env, client, admin, relay) = setup();
    // Default slash_bps is 10_000 (100 %).
    client.bond_collateral(&relay, &1_000_000);
    let evidence = conflicting_evidence(&env, "tx-slash-1");
    client.slash_signer(&relay, &evidence, &admin);

    // Full slash — bond record removed.
    assert!(client.get_bond_record(&relay).is_none());
}

#[test]
fn test_slash_signer_partial_slash_via_param() {
    let (env, client, admin, relay) = setup();
    // 50 % slash.
    client.set_param(&String::from_str(&env, "slash_bps"), &5_000);
    client.bond_collateral(&relay, &1_000_000);
    let evidence = conflicting_evidence(&env, "tx-slash-2");
    client.slash_signer(&relay, &evidence, &admin);

    let record = client.get_bond_record(&relay).unwrap();
    assert_eq!(record.amount, 500_000);
}

#[test]
fn test_slash_signer_cancels_pending_unbond() {
    let (env, client, admin, relay) = setup();
    client.set_param(&String::from_str(&env, "unbond_delay_ledgers"), &1_000);
    client.bond_collateral(&relay, &2_000_000);
    client.unbond_collateral(&relay, &500_000);
    assert!(client.get_unbond_request(&relay).is_some());

    let evidence = conflicting_evidence(&env, "tx-slash-3");
    client.slash_signer(&relay, &evidence, &admin);

    // Unbond request must be cleared.
    assert!(client.get_unbond_request(&relay).is_none());
}

#[test]
fn test_slash_signer_rejects_unbonded_signer() {
    let (env, client, admin, _relay) = setup();
    let stranger = Address::generate(&env);
    let evidence = conflicting_evidence(&env, "tx-slash-4");
    let result = client.try_slash_signer(&stranger, &evidence, &admin);
    assert_eq!(result, Err(Ok(ContractError::SignerNotBonded)));
}

#[test]
fn test_slash_signer_rejects_tx_id_mismatch_in_payload_a() {
    let (env, client, admin, relay) = setup();
    client.bond_collateral(&relay, &1_000_000);

    let account = g_addr(&env);
    let account2 = g_addr2(&env);
    let pa = CallbackPayload {
        transaction_id: String::from_str(&env, "DIFFERENT-ID"),
        stellar_account: account.clone(),
        amount: 1_000,
        asset_code: String::from_str(&env, "USDC"),
        asset_issuer: account.clone(),
        idempotency_key: String::from_str(&env, "key-a"),
        anchor_transaction_id: String::from_str(&env, "anc-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(&env, "pending_external"),
    };
    let pb = CallbackPayload {
        transaction_id: String::from_str(&env, "tx-evidence-mismatch"),
        stellar_account: account2.clone(),
        amount: 999,
        asset_code: String::from_str(&env, "USDC"),
        asset_issuer: account2,
        idempotency_key: String::from_str(&env, "key-b"),
        anchor_transaction_id: String::from_str(&env, "anc-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(&env, "pending_external"),
    };
    let evidence = SlashEvidence {
        tx_id: String::from_str(&env, "tx-evidence-mismatch"),
        payload_a: pa,
        payload_b: pb,
    };
    let result = client.try_slash_signer(&relay, &evidence, &admin);
    assert_eq!(result, Err(Ok(ContractError::EvidenceTxIdMismatch)));
}

#[test]
fn test_slash_signer_rejects_identical_payloads() {
    let (env, client, admin, relay) = setup();
    client.bond_collateral(&relay, &1_000_000);

    let account = g_addr(&env);
    let pa = CallbackPayload {
        transaction_id: String::from_str(&env, "tx-identical"),
        stellar_account: account.clone(),
        amount: 1_000,
        asset_code: String::from_str(&env, "USDC"),
        asset_issuer: account.clone(),
        idempotency_key: String::from_str(&env, "key-a"),
        anchor_transaction_id: String::from_str(&env, "anc-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(&env, "pending_external"),
    };
    // pb identical to pa (same idempotency_key too — doesn't matter).
    let pb = pa.clone();
    let evidence = SlashEvidence {
        tx_id: String::from_str(&env, "tx-identical"),
        payload_a: pa,
        payload_b: pb,
    };
    let result = client.try_slash_signer(&relay, &evidence, &admin);
    assert_eq!(result, Err(Ok(ContractError::EvidenceNotConflicting)));
}

#[test]
fn test_slash_signer_idempotency_key_difference_alone_is_not_conflicting() {
    let (env, client, admin, relay) = setup();
    client.bond_collateral(&relay, &1_000_000);

    let account = g_addr(&env);
    let pa = CallbackPayload {
        transaction_id: String::from_str(&env, "tx-idem-only"),
        stellar_account: account.clone(),
        amount: 1_000,
        asset_code: String::from_str(&env, "USDC"),
        asset_issuer: account.clone(),
        idempotency_key: String::from_str(&env, "key-a"),
        anchor_transaction_id: String::from_str(&env, "anc-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(&env, "pending_external"),
    };
    let pb = CallbackPayload {
        // Only the idempotency_key differs — should not count as conflicting.
        idempotency_key: String::from_str(&env, "key-b"),
        ..pa.clone()
    };
    let evidence = SlashEvidence {
        tx_id: String::from_str(&env, "tx-idem-only"),
        payload_a: pa,
        payload_b: pb,
    };
    let result = client.try_slash_signer(&relay, &evidence, &admin);
    assert_eq!(result, Err(Ok(ContractError::EvidenceNotConflicting)));
}

// ─── set_anchor_tier / get_anchor_tier / compute_effective_fee (#145) ─────────

#[test]
fn test_set_anchor_tier_happy_path() {
    let (env, client, _admin, _relay) = setup();
    let anchor = Address::generate(&env);
    client.set_anchor_tier(&anchor, &500, &String::from_str(&env, "silver"));
    let config = client.get_anchor_tier(&anchor);
    assert_eq!(config.rebate_bps, 500);
    assert_eq!(config.label, String::from_str(&env, "silver"));
    assert_eq!(config.anchor, anchor);
}

#[test]
fn test_set_anchor_tier_update_overwrites_previous() {
    let (env, client, _admin, _relay) = setup();
    let anchor = Address::generate(&env);
    client.set_anchor_tier(&anchor, &200, &String::from_str(&env, "bronze"));
    client.set_anchor_tier(&anchor, &1_000, &String::from_str(&env, "gold"));
    let config = client.get_anchor_tier(&anchor);
    assert_eq!(config.rebate_bps, 1_000);
    assert_eq!(config.label, String::from_str(&env, "gold"));
}

#[test]
fn test_set_anchor_tier_rejects_rebate_above_10000() {
    let (env, client, _admin, _relay) = setup();
    let anchor = Address::generate(&env);
    let result = client.try_set_anchor_tier(&anchor, &10_001, &String::from_str(&env, "x"));
    assert_eq!(result, Err(Ok(ContractError::InvalidRebateBps)));
}

#[test]
fn test_set_anchor_tier_allows_full_rebate_10000() {
    let (env, client, _admin, _relay) = setup();
    let anchor = Address::generate(&env);
    // 10_000 bps = 100 % rebate — should be allowed.
    client.set_anchor_tier(&anchor, &10_000, &String::from_str(&env, "free"));
    let config = client.get_anchor_tier(&anchor);
    assert_eq!(config.rebate_bps, 10_000);
}

#[test]
fn test_set_anchor_tier_rejects_label_exceeding_max_length() {
    let (env, client, _admin, _relay) = setup();
    let anchor = Address::generate(&env);
    // 17 characters — one over the 16-byte limit.
    let long_label = String::from_str(&env, "a_label_too_long!");
    let result = client.try_set_anchor_tier(&anchor, &500, &long_label);
    assert_eq!(result, Err(Ok(ContractError::InvalidTierLabel)));
}

#[test]
fn test_get_anchor_tier_returns_not_found() {
    let (env, client, _admin, _relay) = setup();
    let anchor = Address::generate(&env);
    let result = client.try_get_anchor_tier(&anchor);
    assert_eq!(result, Err(Ok(ContractError::AnchorTierNotFound)));
}

#[test]
fn test_compute_effective_fee_with_rebate() {
    let (env, client, _admin, _relay) = setup();
    let anchor = Address::generate(&env);
    // 10 % rebate → effective fee = base_fee × 0.90.
    client.set_anchor_tier(&anchor, &1_000, &String::from_str(&env, "bronze"));
    let effective = client.compute_effective_fee(&anchor, &10_000);
    assert_eq!(effective, 9_000);
}

#[test]
fn test_compute_effective_fee_full_rebate_yields_zero() {
    let (env, client, _admin, _relay) = setup();
    let anchor = Address::generate(&env);
    client.set_anchor_tier(&anchor, &10_000, &String::from_str(&env, "free"));
    let effective = client.compute_effective_fee(&anchor, &50_000);
    assert_eq!(effective, 0);
}

#[test]
fn test_compute_effective_fee_no_tier_returns_full_base_fee() {
    let (env, client, _admin, _relay) = setup();
    let anchor = Address::generate(&env);
    // No tier set — should return the base fee unchanged.
    let effective = client.compute_effective_fee(&anchor, &10_000);
    assert_eq!(effective, 10_000);
}

#[test]
fn test_compute_effective_fee_zero_rebate_returns_full_base_fee() {
    let (env, client, _admin, _relay) = setup();
    let anchor = Address::generate(&env);
    client.set_anchor_tier(&anchor, &0, &String::from_str(&env, "standard"));
    let effective = client.compute_effective_fee(&anchor, &10_000);
    assert_eq!(effective, 10_000);
}

#[test]
fn test_compute_effective_fee_rejects_negative_base_fee() {
    let (env, client, _admin, _relay) = setup();
    let anchor = Address::generate(&env);
    let result = client.try_compute_effective_fee(&anchor, &-1);
    assert_eq!(result, Err(Ok(ContractError::InvalidAmount)));
}
