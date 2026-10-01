//! # Boundary-value coverage for every numeric / length cap (#134)
//!
//! One section per cap in the inventory below. Every cap gets paired tests at
//! the limit (allowed) and one past it (rejected), plus the lower-bound
//! equivalents where the cap has one. Every test goes through the public
//! entry point rather than calling [`crate::validation::Validator`] directly,
//! so it also proves the guard is actually wired into that entry point.
//!
//! ## Cap inventory
//!
//! | #   | Cap | Where | Lower bound | Upper bound |
//! |-----|-----|-------|-------------|-------------|
//! | C1  | `stellar_account` length | `validation.rs` | 56 (55 ✗) | 56 (57 ✗) |
//! | C2  | `amount` | `validation.rs` | 1 (0, −1 ✗) | `i128::MAX` ✓ |
//! | C3  | `asset_code` length | `validation.rs` | 1 (0 ✗) | 12 (13 ✗) |
//! | C4  | `asset_issuer` length | `validation.rs` | 56 (55 ✗) | 56 (57 ✗) |
//! | C5  | `idempotency_key` length | `validation.rs` | 1 (0 ✗) | none (finding F-A) |
//! | C6  | `transaction_id` length | `validation.rs` | none, 0 ✓ (finding F-B) | 64 (65 ✗) |
//! | C7  | `anchor_transaction_id` length | `validation.rs` | none, 0 ✓ | 64 (65 ✗) |
//! | C8  | `callback_status` length | `validation.rs` | none, 0 ✓ | 32 (33 ✗) |
//! | C9  | `stellar_tx_hash` length | `validation.rs` | none, 0 ✓ | 72 (73 ✗) |
//! | C10 | `failure_reason` length | `validation.rs` | none, 0 ✓ | 64 (65 ✗) |
//! | C11 | param name length | `lib.rs` `MAX_PARAM_NAME_LEN` | 1 (0 ✗) | 32 (33 ✗) |
//! | C12 | anchor tier label length | `lib.rs` `MAX_TIER_LABEL_LEN` | none, 0 ✓ | 16 (17 ✗) |
//! | C13 | `rebate_bps` | `lib.rs` | 0 ✓ (`u32`) | 10 000 (10 001 ✗) |
//! | C14 | bond amount | `lib.rs` | 1 (0, −1 ✗) | running total ≤ `i128::MAX` (overflow reverts) |
//! | C15 | unbond amount | `lib.rs` | 1 (0 ✗) | bonded balance (+1 ✗) |
//! | C16 | unbond claim ledger | `lib.rs` | `claimable_at` ✓ (−1 ✗) | — |
//! | C17 | `unbond_delay_ledgers` → `u32` | `lib.rs` | clamped to 0 | clamped to `u32::MAX` (**fixed**, was wrapping) |
//! | C18 | `slash_bps` | `lib.rs` | clamped to 0 | clamped to 10 000 |
//! | C19 | slash product `amount × bps` | `lib.rs` | — | `amount ≤ i128::MAX / 10 000` (finding F-C) |
//! | C20 | `base_fee` | `lib.rs` | 0 ✓ (−1 ✗) | `base_fee ≤ i128::MAX / 10 000` (finding F-C) |
//!
//! ## Findings (tracked, not fixed here — this issue adds no new caps)
//!
//! * **F-A**: `idempotency_key` has no maximum length. It is stored as a
//!   temporary-storage key, so its size drives rent cost the same way the
//!   capped fields do.
//! * **F-B**: `transaction_id` (and the other max-only string fields) accept
//!   the empty string. An empty `transaction_id` registers a record keyed by
//!   `""`.
//! * **F-C**: `slash_signer` and `compute_effective_fee` multiply by up to
//!   10 000 before dividing. Above `i128::MAX / 10 000` the multiply
//!   overflows and the call reverts (`overflow-checks = true`), so a bond that
//!   large cannot be slashed. The checked-arithmetic work is #152.
//!
//! C17 was a real bug. `unbond_delay_ledgers = 2^32` wrapped to a zero delay,
//! so collateral could be withdrawn immediately and dodge a pending slash.
//! The conversion now clamps; see `unbond_collateral`.

#![cfg(test)]

extern crate std;

use std::string::String as StdString;

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String,
};

use crate::types::{CallbackPayload, CallbackType, ContractError, SlashEvidence};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

// ─── Helpers ─────────────────────────────────────────────────────────────────

const G_ADDR: &str = "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ";

struct Ctx {
    env: Env,
    client: SynapseCoreContractClient<'static>,
    admin: Address,
    relay: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    let id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    Ctx {
        env,
        client,
        admin,
        relay,
    }
}

/// A `String` of exactly `n` bytes.
fn str_of_len(env: &Env, n: usize) -> String {
    String::from_str(env, &"a".repeat(n))
}

fn s(env: &Env, v: &str) -> String {
    String::from_str(env, v)
}

fn payload(env: &Env) -> CallbackPayload {
    CallbackPayload {
        transaction_id: s(env, "tx-1"),
        stellar_account: s(env, G_ADDR),
        amount: 1_000,
        asset_code: s(env, "USDC"),
        asset_issuer: s(env, G_ADDR),
        idempotency_key: s(env, "idem-1"),
        anchor_transaction_id: s(env, "anchor-1"),
        callback_type: CallbackType::Deposit,
        callback_status: s(env, "pending_external"),
    }
}

/// Register a payload after applying `edit`, returning the contract result.
fn register_with(c: &Ctx, edit: impl FnOnce(&mut CallbackPayload)) -> Result<(), ContractError> {
    let mut p = payload(&c.env);
    edit(&mut p);
    match c.client.try_register_callback(&p) {
        Ok(Ok(_)) => Ok(()),
        Err(Ok(e)) => Err(e),
        other => panic!("unexpected host-level result: {other:?}"),
    }
}

/// `G_ADDR` shortened or lengthened by one character.
fn g_addr_len(env: &Env, n: usize) -> String {
    let mut a = StdString::from(G_ADDR);
    while a.len() < n {
        a.push('A');
    }
    a.truncate(n);
    String::from_str(env, &a)
}

fn processing_tx(c: &Ctx) -> String {
    let id = c.client.register_callback(&payload(&c.env));
    c.client.start_processing(&id, &c.relay);
    id
}

fn evidence(env: &Env) -> SlashEvidence {
    let a = payload(env);
    let mut b = payload(env);
    b.amount += 1;
    SlashEvidence {
        tx_id: s(env, "tx-1"),
        payload_a: a,
        payload_b: b,
    }
}

// ─── C1 stellar_account: exactly 56 ─────────────────────────────────────────

#[test]
fn c1_stellar_account_at_56_is_allowed() {
    let c = setup();
    assert_eq!(
        register_with(&c, |p| p.stellar_account = g_addr_len(&c.env, 56)),
        Ok(())
    );
}

#[test]
fn c1_stellar_account_at_57_is_rejected() {
    let c = setup();
    assert_eq!(
        register_with(&c, |p| p.stellar_account = g_addr_len(&c.env, 57)),
        Err(ContractError::InvalidStellarAccount)
    );
}

#[test]
fn c1_stellar_account_at_55_and_0_are_rejected() {
    let c = setup();
    for n in [55, 0] {
        assert_eq!(
            register_with(&c, |p| p.stellar_account = g_addr_len(&c.env, n)),
            Err(ContractError::InvalidStellarAccount),
            "len {n}"
        );
    }
}

// ─── C2 amount: > 0 ─────────────────────────────────────────────────────────

#[test]
fn c2_amount_at_1_and_max_are_allowed() {
    let c = setup();
    assert_eq!(register_with(&c, |p| p.amount = 1), Ok(()));
    assert_eq!(
        register_with(&c, |p| {
            p.amount = i128::MAX;
            p.transaction_id = s(&c.env, "tx-max");
            p.idempotency_key = s(&c.env, "idem-max");
        }),
        Ok(())
    );
}

#[test]
fn c2_amount_at_0_and_negative_are_rejected() {
    let c = setup();
    for amount in [0, -1, i128::MIN] {
        assert_eq!(
            register_with(&c, |p| p.amount = amount),
            Err(ContractError::InvalidAmount),
            "amount {amount}"
        );
    }
}

// ─── C3 asset_code: 1..=12 ──────────────────────────────────────────────────

#[test]
fn c3_asset_code_at_1_and_12_are_allowed() {
    let c = setup();
    assert_eq!(register_with(&c, |p| p.asset_code = s(&c.env, "X")), Ok(()));
    assert_eq!(
        register_with(&c, |p| {
            p.asset_code = s(&c.env, "ABCDEFGHIJKL");
            p.transaction_id = s(&c.env, "tx-12");
            p.idempotency_key = s(&c.env, "idem-12");
        }),
        Ok(())
    );
}

#[test]
fn c3_asset_code_at_0_and_13_are_rejected() {
    let c = setup();
    for code in ["", "ABCDEFGHIJKLM"] {
        assert_eq!(
            register_with(&c, |p| p.asset_code = s(&c.env, code)),
            Err(ContractError::InvalidAssetCode),
            "code {code:?}"
        );
    }
}

// ─── C4 asset_issuer: exactly 56 ────────────────────────────────────────────

#[test]
fn c4_asset_issuer_at_56_is_allowed() {
    let c = setup();
    assert_eq!(
        register_with(&c, |p| p.asset_issuer = g_addr_len(&c.env, 56)),
        Ok(())
    );
}

#[test]
fn c4_asset_issuer_at_55_and_57_are_rejected() {
    let c = setup();
    for n in [55, 57] {
        assert_eq!(
            register_with(&c, |p| p.asset_issuer = g_addr_len(&c.env, n)),
            Err(ContractError::InvalidAssetIssuer),
            "len {n}"
        );
    }
}

// ─── C5 idempotency_key: ≥ 1, no upper cap (F-A) ────────────────────────────

#[test]
fn c5_idempotency_key_at_1_is_allowed() {
    let c = setup();
    assert_eq!(
        register_with(&c, |p| p.idempotency_key = str_of_len(&c.env, 1)),
        Ok(())
    );
}

#[test]
fn c5_idempotency_key_at_0_is_rejected() {
    let c = setup();
    assert_eq!(
        register_with(&c, |p| p.idempotency_key = str_of_len(&c.env, 0)),
        Err(ContractError::MissingIdempotencyKey)
    );
}

/// Finding F-A: pins the *absence* of an upper cap so that adding one later
/// has to update this test deliberately.
#[test]
fn c5_idempotency_key_has_no_upper_cap_finding_f_a() {
    let c = setup();
    assert_eq!(
        register_with(&c, |p| p.idempotency_key = str_of_len(&c.env, 1_024)),
        Ok(())
    );
}

// ─── C6–C8 payload string caps ──────────────────────────────────────────────

#[test]
fn c6_transaction_id_at_64_is_allowed() {
    let c = setup();
    assert_eq!(
        register_with(&c, |p| p.transaction_id = str_of_len(&c.env, 64)),
        Ok(())
    );
}

#[test]
fn c6_transaction_id_at_65_is_rejected() {
    let c = setup();
    assert_eq!(
        register_with(&c, |p| p.transaction_id = str_of_len(&c.env, 65)),
        Err(ContractError::StringTooLong)
    );
}

/// Finding F-B: no lower bound. Pinned so a future `≥ 1` rule is a
/// deliberate, visible change.
#[test]
fn c6_transaction_id_at_0_is_allowed_finding_f_b() {
    let c = setup();
    assert_eq!(
        register_with(&c, |p| p.transaction_id = str_of_len(&c.env, 0)),
        Ok(())
    );
}

#[test]
fn c7_anchor_transaction_id_at_0_and_64_are_allowed() {
    let c = setup();
    assert_eq!(
        register_with(&c, |p| p.anchor_transaction_id = str_of_len(&c.env, 64)),
        Ok(())
    );
    assert_eq!(
        register_with(&c, |p| {
            p.anchor_transaction_id = str_of_len(&c.env, 0);
            p.transaction_id = s(&c.env, "tx-2");
            p.idempotency_key = s(&c.env, "idem-2");
        }),
        Ok(())
    );
}

#[test]
fn c7_anchor_transaction_id_at_65_is_rejected() {
    let c = setup();
    assert_eq!(
        register_with(&c, |p| p.anchor_transaction_id = str_of_len(&c.env, 65)),
        Err(ContractError::StringTooLong)
    );
}

#[test]
fn c8_callback_status_at_0_and_32_are_allowed() {
    let c = setup();
    assert_eq!(
        register_with(&c, |p| p.callback_status = str_of_len(&c.env, 32)),
        Ok(())
    );
    assert_eq!(
        register_with(&c, |p| {
            p.callback_status = str_of_len(&c.env, 0);
            p.transaction_id = s(&c.env, "tx-2");
            p.idempotency_key = s(&c.env, "idem-2");
        }),
        Ok(())
    );
}

#[test]
fn c8_callback_status_at_33_is_rejected() {
    let c = setup();
    assert_eq!(
        register_with(&c, |p| p.callback_status = str_of_len(&c.env, 33)),
        Err(ContractError::StringTooLong)
    );
}

// ─── C9 stellar_tx_hash (complete_transaction) ──────────────────────────────

#[test]
fn c9_stellar_tx_hash_at_72_is_allowed() {
    let c = setup();
    let id = processing_tx(&c);
    let r = c
        .client
        .try_complete_transaction(&id, &str_of_len(&c.env, 72), &c.relay);
    assert_eq!(r, Ok(Ok(())));
}

#[test]
fn c9_stellar_tx_hash_at_0_is_allowed() {
    let c = setup();
    let id = processing_tx(&c);
    let r = c
        .client
        .try_complete_transaction(&id, &str_of_len(&c.env, 0), &c.relay);
    assert_eq!(r, Ok(Ok(())));
}

#[test]
fn c9_stellar_tx_hash_at_73_is_rejected_without_state_change() {
    let c = setup();
    let id = processing_tx(&c);
    let r = c
        .client
        .try_complete_transaction(&id, &str_of_len(&c.env, 73), &c.relay);
    assert_eq!(r, Err(Ok(ContractError::StringTooLong)));
    assert_eq!(
        c.client.get_status(&id),
        crate::types::TransactionStatus::Processing
    );
}

// ─── C10 failure_reason (fail_transaction) ──────────────────────────────────

#[test]
fn c10_failure_reason_at_0_and_64_are_allowed() {
    let c = setup();
    for (tx, n) in [("tx-a", 64), ("tx-b", 0)] {
        let mut p = payload(&c.env);
        p.transaction_id = s(&c.env, tx);
        p.idempotency_key = s(&c.env, tx);
        let id = c.client.register_callback(&p);
        let r = c
            .client
            .try_fail_transaction(&id, &str_of_len(&c.env, n), &c.relay);
        assert_eq!(r, Ok(Ok(())), "len {n}");
    }
}

#[test]
fn c10_failure_reason_at_65_is_rejected() {
    let c = setup();
    let id = c.client.register_callback(&payload(&c.env));
    let r = c
        .client
        .try_fail_transaction(&id, &str_of_len(&c.env, 65), &c.relay);
    assert_eq!(r, Err(Ok(ContractError::StringTooLong)));
}

// ─── C11 param name: 1..=32 ─────────────────────────────────────────────────

#[test]
fn c11_param_name_at_1_and_32_are_allowed() {
    let c = setup();
    for n in [1, 32] {
        assert_eq!(
            c.client.try_set_param(&str_of_len(&c.env, n), &7),
            Ok(Ok(())),
            "len {n}"
        );
    }
}

#[test]
fn c11_param_name_at_0_and_33_are_rejected() {
    let c = setup();
    for n in [0, 33] {
        assert_eq!(
            c.client.try_set_param(&str_of_len(&c.env, n), &7),
            Err(Ok(ContractError::InvalidParamName)),
            "len {n}"
        );
    }
}

// ─── C12 tier label: ≤ 16 ───────────────────────────────────────────────────

#[test]
fn c12_tier_label_at_0_and_16_are_allowed() {
    let c = setup();
    let anchor = Address::generate(&c.env);
    for n in [0, 16] {
        assert_eq!(
            c.client
                .try_set_anchor_tier(&anchor, &100, &str_of_len(&c.env, n)),
            Ok(Ok(())),
            "len {n}"
        );
    }
}

#[test]
fn c12_tier_label_at_17_is_rejected() {
    let c = setup();
    let anchor = Address::generate(&c.env);
    assert_eq!(
        c.client
            .try_set_anchor_tier(&anchor, &100, &str_of_len(&c.env, 17)),
        Err(Ok(ContractError::InvalidTierLabel))
    );
}

// ─── C13 rebate_bps: ≤ 10_000 ───────────────────────────────────────────────

#[test]
fn c13_rebate_bps_at_0_and_10000_are_allowed() {
    let c = setup();
    let anchor = Address::generate(&c.env);
    for bps in [0u32, 10_000] {
        assert_eq!(
            c.client.try_set_anchor_tier(&anchor, &bps, &s(&c.env, "t")),
            Ok(Ok(())),
            "bps {bps}"
        );
    }
    // At 10_000 the effective fee is exactly zero.
    assert_eq!(c.client.compute_effective_fee(&anchor, &12_345), 0);
}

#[test]
fn c13_rebate_bps_at_10001_is_rejected() {
    let c = setup();
    let anchor = Address::generate(&c.env);
    assert_eq!(
        c.client
            .try_set_anchor_tier(&anchor, &10_001, &s(&c.env, "t")),
        Err(Ok(ContractError::InvalidRebateBps))
    );
}

// ─── C14 bond amount: > 0, total ≤ i128::MAX ────────────────────────────────

#[test]
fn c14_bond_amount_at_1_is_allowed() {
    let c = setup();
    assert_eq!(c.client.try_bond_collateral(&c.relay, &1), Ok(Ok(())));
}

#[test]
fn c14_bond_amount_at_0_and_negative_are_rejected() {
    let c = setup();
    for amount in [0, -1] {
        assert_eq!(
            c.client.try_bond_collateral(&c.relay, &amount),
            Err(Ok(ContractError::InvalidBondAmount)),
            "amount {amount}"
        );
    }
}

#[test]
fn c14_bond_total_at_i128_max_is_allowed_and_one_more_reverts() {
    let c = setup();
    c.client.bond_collateral(&c.relay, &(i128::MAX - 1));
    assert_eq!(c.client.try_bond_collateral(&c.relay, &1), Ok(Ok(())));
    assert!(
        c.client.try_bond_collateral(&c.relay, &1).is_err(),
        "a top-up past i128::MAX must revert (overflow-checks), not wrap"
    );
    assert_eq!(
        c.client.get_bond_record(&c.relay).unwrap().amount,
        i128::MAX
    );
}

// ─── C15 unbond amount: 1..=bonded ──────────────────────────────────────────

#[test]
fn c15_unbond_at_bonded_balance_is_allowed() {
    let c = setup();
    c.client.bond_collateral(&c.relay, &500);
    assert_eq!(c.client.try_unbond_collateral(&c.relay, &500), Ok(Ok(())));
}

#[test]
fn c15_unbond_at_1_is_allowed() {
    let c = setup();
    c.client.bond_collateral(&c.relay, &500);
    assert_eq!(c.client.try_unbond_collateral(&c.relay, &1), Ok(Ok(())));
}

#[test]
fn c15_unbond_one_past_bonded_balance_is_rejected() {
    let c = setup();
    c.client.bond_collateral(&c.relay, &500);
    assert_eq!(
        c.client.try_unbond_collateral(&c.relay, &501),
        Err(Ok(ContractError::InsufficientBond))
    );
}

#[test]
fn c15_unbond_at_0_is_rejected() {
    let c = setup();
    c.client.bond_collateral(&c.relay, &500);
    assert_eq!(
        c.client.try_unbond_collateral(&c.relay, &0),
        Err(Ok(ContractError::InvalidBondAmount))
    );
}

// ─── C16 claim ledger: ≥ claimable_at ───────────────────────────────────────

#[test]
fn c16_claim_one_ledger_early_is_rejected_and_at_claimable_is_allowed() {
    let c = setup();
    c.env.ledger().set_sequence_number(1_000);
    c.client.set_param(&s(&c.env, "unbond_delay_ledgers"), &50);
    c.client.bond_collateral(&c.relay, &500);
    c.client.unbond_collateral(&c.relay, &200);

    c.env.ledger().set_sequence_number(1_049);
    assert_eq!(
        c.client.try_claim_unbond(&c.relay),
        Err(Ok(ContractError::UnbondDelayNotElapsed))
    );

    c.env.ledger().set_sequence_number(1_050);
    assert_eq!(c.client.try_claim_unbond(&c.relay), Ok(Ok(())));
    assert_eq!(c.client.get_bond_record(&c.relay).unwrap().amount, 300);
}

// ─── C17 unbond delay param → u32 (fixed) ───────────────────────────────────

fn claimable_at_for_delay(delay: i128) -> u32 {
    let c = setup();
    c.env.ledger().set_sequence_number(1_000);
    c.client
        .set_param(&s(&c.env, "unbond_delay_ledgers"), &delay);
    c.client.bond_collateral(&c.relay, &500);
    c.client.unbond_collateral(&c.relay, &100);
    c.client
        .get_unbond_request(&c.relay)
        .unwrap()
        .claimable_at_ledger
}

#[test]
fn c17_delay_at_0_is_claimable_immediately() {
    assert_eq!(claimable_at_for_delay(0), 1_000);
}

#[test]
fn c17_delay_at_u32_max_saturates() {
    assert_eq!(claimable_at_for_delay(u32::MAX as i128), u32::MAX);
}

/// Regression for the wrapping cast: 2^32 used to become a zero delay
/// (claimable immediately); it must saturate like any other oversized delay.
#[test]
fn c17_delay_one_past_u32_max_saturates_instead_of_wrapping() {
    assert_eq!(claimable_at_for_delay(u32::MAX as i128 + 1), u32::MAX);
    assert_eq!(claimable_at_for_delay(i128::MAX), u32::MAX);
}

/// Negative delays clamp to zero instead of wrapping to a huge `u32`.
#[test]
fn c17_negative_delay_clamps_to_zero() {
    assert_eq!(claimable_at_for_delay(-1), 1_000);
    assert_eq!(claimable_at_for_delay(i128::MIN), 1_000);
}

// ─── C18 slash_bps clamp: [0, 10_000] ───────────────────────────────────────

fn slashed_with_bps(bps: Option<i128>) -> (i128, Option<i128>) {
    let c = setup();
    if let Some(bps) = bps {
        c.client.set_param(&s(&c.env, "slash_bps"), &bps);
    }
    c.client.bond_collateral(&c.relay, &10_000);
    c.client.slash_signer(&c.relay, &evidence(&c.env), &c.admin);
    let remaining = c.client.get_bond_record(&c.relay).map(|r| r.amount);
    (10_000 - remaining.unwrap_or(0), remaining)
}

#[test]
fn c18_slash_bps_at_0_and_10000_are_exact() {
    assert_eq!(slashed_with_bps(Some(0)), (0, Some(10_000)));
    assert_eq!(slashed_with_bps(Some(10_000)), (10_000, None));
    assert_eq!(slashed_with_bps(None), (10_000, None), "default is 100 %");
}

#[test]
fn c18_slash_bps_one_past_either_bound_is_clamped() {
    assert_eq!(slashed_with_bps(Some(-1)), (0, Some(10_000)));
    assert_eq!(slashed_with_bps(Some(10_001)), (10_000, None));
}

// ─── C19 / C20 multiply-before-divide headroom (finding F-C) ────────────────

const MUL_HEADROOM: i128 = i128::MAX / 10_000;

#[test]
fn c19_slash_at_mul_headroom_is_allowed() {
    let c = setup();
    c.client.bond_collateral(&c.relay, &MUL_HEADROOM);
    assert_eq!(
        c.client
            .try_slash_signer(&c.relay, &evidence(&c.env), &c.admin),
        Ok(Ok(()))
    );
}

/// Finding F-C (#152): one past the headroom the multiply overflows and the
/// slash reverts. It fails closed (the bond stays intact), but it also means
/// a bond this large cannot be slashed.
#[test]
fn c19_slash_one_past_mul_headroom_reverts_finding_f_c() {
    let c = setup();
    c.client.bond_collateral(&c.relay, &(MUL_HEADROOM + 1));
    assert!(c
        .client
        .try_slash_signer(&c.relay, &evidence(&c.env), &c.admin)
        .is_err());
    assert_eq!(
        c.client.get_bond_record(&c.relay).unwrap().amount,
        MUL_HEADROOM + 1
    );
}

#[test]
fn c20_base_fee_at_0_is_allowed_and_negative_is_rejected() {
    let c = setup();
    let anchor = Address::generate(&c.env);
    assert_eq!(c.client.try_compute_effective_fee(&anchor, &0), Ok(Ok(0)));
    assert_eq!(
        c.client.try_compute_effective_fee(&anchor, &-1),
        Err(Ok(ContractError::InvalidAmount))
    );
}

#[test]
fn c20_base_fee_at_mul_headroom_is_allowed_and_one_past_reverts_finding_f_c() {
    let c = setup();
    let anchor = Address::generate(&c.env);
    c.client.set_anchor_tier(&anchor, &0, &s(&c.env, "t"));
    assert_eq!(
        c.client.try_compute_effective_fee(&anchor, &MUL_HEADROOM),
        Ok(Ok(MUL_HEADROOM))
    );
    assert!(c
        .client
        .try_compute_effective_fee(&anchor, &(MUL_HEADROOM + 1))
        .is_err());
    // Without a tier there is no multiply, so the full range is accepted.
    let untiered = Address::generate(&c.env);
    assert_eq!(
        c.client.try_compute_effective_fee(&untiered, &i128::MAX),
        Ok(Ok(i128::MAX))
    );
}
