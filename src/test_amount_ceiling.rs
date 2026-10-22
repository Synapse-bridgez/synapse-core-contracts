//! Tests for the global `global_max_amount` ceiling (#169) and its
//! precedence with per-anchor ceilings: whichever is lower wins.

#![cfg(test)]

extern crate std;

use soroban_sdk::{testutils::Address as _, Address, Env, String, Vec};

use crate::types::{CallbackPayload, CallbackType, ContractError, DEFAULT_GLOBAL_MAX_AMOUNT};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

/// Two distinct, checksum-valid G-addresses used as anchors (`asset_issuer`).
const ANCHOR_A: &str = "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ";
const ANCHOR_B: &str = "GADQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOZPI";

fn setup() -> (Env, SynapseCoreContractClient<'static>, Address) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    (env, client, relay)
}

fn payload(env: &Env, tx_id: &str, anchor: &str, amount: i128) -> CallbackPayload {
    CallbackPayload {
        transaction_id: String::from_str(env, tx_id),
        stellar_account: String::from_str(env, ANCHOR_A),
        amount,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: String::from_str(env, anchor),
        idempotency_key: String::from_str(env, tx_id),
        anchor_transaction_id: String::from_str(env, "anchor-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    }
}

fn set_global(env: &Env, client: &SynapseCoreContractClient, value: i128) {
    client.set_param(&String::from_str(env, "global_max_amount"), &value);
}

/// Register at `ceiling` (must succeed) and `ceiling + 1` (must fail).
fn assert_boundary(env: &Env, client: &SynapseCoreContractClient, anchor: &str, ceiling: i128) {
    let tag = if anchor == ANCHOR_A { "a" } else { "b" };
    let at = std::format!("at-{tag}-{ceiling}");
    let over = std::format!("over-{tag}-{ceiling}");
    assert!(client
        .try_register_callback(&payload(env, &at, anchor, ceiling))
        .is_ok());
    assert_eq!(
        client.try_register_callback(&payload(env, &over, anchor, ceiling + 1)),
        Err(Ok(ContractError::AmountCeilingExceeded))
    );
}

#[test]
fn global_ceiling_is_on_by_default() {
    let (env, client, _relay) = setup();
    assert_eq!(client.get_global_max_amount(), DEFAULT_GLOBAL_MAX_AMOUNT);
    assert_eq!(
        client.get_amount_ceiling(&String::from_str(&env, ANCHOR_A)),
        DEFAULT_GLOBAL_MAX_AMOUNT
    );
    assert_boundary(&env, &client, ANCHOR_A, DEFAULT_GLOBAL_MAX_AMOUNT);
}

#[test]
fn configured_global_ceiling_exact_boundary() {
    let (env, client, _relay) = setup();
    set_global(&env, &client, 5_000);
    assert_eq!(client.get_global_max_amount(), 5_000);
    assert_boundary(&env, &client, ANCHOR_A, 5_000);
    // Applies to unconfigured anchors too.
    assert_boundary(&env, &client, ANCHOR_B, 5_000);
}

#[test]
fn per_anchor_ceiling_wins_when_stricter() {
    let (env, client, _relay) = setup();
    set_global(&env, &client, 10_000);
    client.set_anchor_amount_ceiling(&String::from_str(&env, ANCHOR_A), &3_000);

    assert_eq!(
        client.get_amount_ceiling(&String::from_str(&env, ANCHOR_A)),
        3_000
    );
    assert_boundary(&env, &client, ANCHOR_A, 3_000);
    // The other anchor is only bound by the global ceiling.
    assert_boundary(&env, &client, ANCHOR_B, 10_000);
}

#[test]
fn global_ceiling_wins_when_stricter() {
    let (env, client, _relay) = setup();
    client.set_anchor_amount_ceiling(&String::from_str(&env, ANCHOR_A), &10_000);
    set_global(&env, &client, 4_000);

    assert_eq!(
        client.get_amount_ceiling(&String::from_str(&env, ANCHOR_A)),
        4_000
    );
    // 4_001 is under the per-anchor ceiling but still rejected.
    assert_boundary(&env, &client, ANCHOR_A, 4_000);
}

#[test]
fn raising_global_ceiling_restores_per_anchor_precedence() {
    let (env, client, _relay) = setup();
    client.set_anchor_amount_ceiling(&String::from_str(&env, ANCHOR_A), &6_000);
    set_global(&env, &client, 2_000);
    assert_boundary(&env, &client, ANCHOR_A, 2_000);

    set_global(&env, &client, 50_000);
    assert_boundary(&env, &client, ANCHOR_A, 6_000);
}

#[test]
fn global_ceiling_enforced_on_batch_registration() {
    let (env, client, relay) = setup();
    set_global(&env, &client, 1_000);
    let mut v = Vec::new(&env);
    v.push_back(payload(&env, "bt-1", ANCHOR_A, 1_000));
    v.push_back(payload(&env, "bt-2", ANCHOR_B, 1_001));
    assert_eq!(
        client.try_batch_register_callback(&v, &relay),
        Err(Ok(ContractError::AmountCeilingExceeded))
    );
    assert!(client
        .try_get_status(&String::from_str(&env, "bt-1"))
        .is_err());
}

#[test]
fn global_ceiling_must_be_positive() {
    let (env, client, _relay) = setup();
    let name = String::from_str(&env, "global_max_amount");
    for bad in [0, -1] {
        assert_eq!(
            client.try_set_param(&name, &bad),
            Err(Ok(ContractError::InvalidParamValue))
        );
    }
    assert_eq!(client.get_global_max_amount(), DEFAULT_GLOBAL_MAX_AMOUNT);
}

#[test]
fn per_anchor_ceiling_must_be_positive() {
    let (env, client, _relay) = setup();
    assert_eq!(
        client.try_set_anchor_amount_ceiling(&String::from_str(&env, ANCHOR_A), &0),
        Err(Ok(ContractError::InvalidAmount))
    );
}

#[test]
fn ceiling_setters_require_admin_auth() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    client.initialize(&admin, &relay);

    // No auth mocked at all: both setters must refuse.
    assert!(client
        .try_set_param(&String::from_str(&env, "global_max_amount"), &1)
        .is_err());
    assert!(client
        .try_set_anchor_amount_ceiling(&String::from_str(&env, ANCHOR_A), &1)
        .is_err());
}

// ---------------------------------------------------------------------------
// Global default ceiling: unset default and at/above/below an admin value.
// ---------------------------------------------------------------------------

#[test]
fn unset_global_ceiling_falls_back_to_documented_default() {
    let (env, client, _relay) = setup();
    // "Unset" is not "no ceiling": the documented default applies.
    assert_eq!(client.get_global_max_amount(), DEFAULT_GLOBAL_MAX_AMOUNT);
    assert_boundary(&env, &client, ANCHOR_A, DEFAULT_GLOBAL_MAX_AMOUNT);
}

#[test]
fn configured_global_ceiling_below_at_above() {
    let (env, client, _relay) = setup();
    set_global(&env, &client, 7_500);
    // Below.
    assert!(client
        .try_register_callback(&payload(&env, "g-below", ANCHOR_A, 7_499))
        .is_ok());
    // At.
    assert!(client
        .try_register_callback(&payload(&env, "g-at", ANCHOR_A, 7_500))
        .is_ok());
    // Above.
    assert_eq!(
        client.try_register_callback(&payload(&env, "g-above", ANCHOR_A, 7_501)),
        Err(Ok(ContractError::AmountCeilingExceeded))
    );
}

// ---------------------------------------------------------------------------
// Per-anchor overrides: at/above/below the override and the global fallback.
// ---------------------------------------------------------------------------

#[test]
fn per_anchor_override_below_at_above() {
    let (env, client, _relay) = setup();
    set_global(&env, &client, 10_000);
    client.set_anchor_amount_ceiling(&String::from_str(&env, ANCHOR_A), &2_000);
    // Below the override.
    assert!(client
        .try_register_callback(&payload(&env, "a-below", ANCHOR_A, 1_999))
        .is_ok());
    // At the override.
    assert!(client
        .try_register_callback(&payload(&env, "a-at", ANCHOR_A, 2_000))
        .is_ok());
    // Above the override (still under the global ceiling).
    assert_eq!(
        client.try_register_callback(&payload(&env, "a-above", ANCHOR_A, 2_001)),
        Err(Ok(ContractError::AmountCeilingExceeded))
    );
    // The unconfigured anchor is bound only by the global fallback.
    assert!(client
        .try_register_callback(&payload(&env, "b-below", ANCHOR_B, 9_999))
        .is_ok());
    assert!(client
        .try_register_callback(&payload(&env, "b-at", ANCHOR_B, 10_000))
        .is_ok());
    assert_eq!(
        client.try_register_callback(&payload(&env, "b-above", ANCHOR_B, 10_001)),
        Err(Ok(ContractError::AmountCeilingExceeded))
    );
}

// ---------------------------------------------------------------------------
// Exhaustive 2^8 invalid-field combination test (COST_MODEL.md §12.5).
//
// Eight independently invalidatable fields. For every one of the 256 subsets
// we assert (a) the accept/reject outcome is unchanged versus the baseline
// (reject iff at least one field is invalid) and (b) the first-reported error
// follows the documented cheapest-first order:
//   amount -> host-length checks -> asset_code -> stellar_account/asset_issuer
// ---------------------------------------------------------------------------

/// The eight invalidatable fields, in documented cheapest-first order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BadField {
    Amount,
    TxIdTooLong,
    AnchorTxIdTooLong,
    AssetCode,
    StellarAccount,
    AssetIssuer,
    IdempotencyKeyEmpty,
    CallbackStatus,
}

const BAD_FIELDS: [BadField; 8] = [
    BadField::Amount,
    BadField::TxIdTooLong,
    BadField::AnchorTxIdTooLong,
    BadField::AssetCode,
    BadField::StellarAccount,
    BadField::AssetIssuer,
    BadField::IdempotencyKeyEmpty,
    BadField::CallbackStatus,
];

fn valid_payload(env: &Env, tx_id: &str) -> CallbackPayload {
    payload(env, tx_id, ANCHOR_A, 100)
}

fn apply_bad(env: &Env, p: &mut CallbackPayload, field: BadField) {
    match field {
        BadField::Amount => p.amount = -1,
        BadField::TxIdTooLong => p.transaction_id = String::from_str(env, &"x".repeat(200)),
        BadField::AnchorTxIdTooLong => {
            p.anchor_transaction_id = String::from_str(env, &"y".repeat(200))
        }
        BadField::AssetCode => p.asset_code = String::from_str(env, "TOOLONGCODE"),
        BadField::StellarAccount => p.stellar_account = String::from_str(env, "not-a-strkey"),
        BadField::AssetIssuer => p.asset_issuer = String::from_str(env, "not-a-strkey"),
        BadField::IdempotencyKeyEmpty => p.idempotency_key = String::from_str(env, ""),
        BadField::CallbackStatus => p.callback_status = String::from_str(env, ""),
    }
}

/// Expected first error for a single invalid field, per the documented order.
fn expected_error(field: BadField) -> ContractError {
    match field {
        BadField::Amount => ContractError::InvalidAmount,
        BadField::TxIdTooLong | BadField::AnchorTxIdTooLong => ContractError::InvalidPayload,
        BadField::AssetCode => ContractError::InvalidAssetCode,
        BadField::StellarAccount | BadField::AssetIssuer => ContractError::InvalidStellarAccount,
        BadField::IdempotencyKeyEmpty => ContractError::InvalidPayload,
        BadField::CallbackStatus => ContractError::InvalidPayload,
    }
}

#[test]
fn exhaustive_2_pow_8_invalid_field_combinations() {
    let (env, client, _relay) = setup();
    // Generous ceiling so amount-ceiling never masks the field checks.
    set_global(&env, &client, 1_000_000);

    for mask in 0u32..256 {
        let mut p = valid_payload(&env, &std::format!("combo-{mask}"));
        let mut first: Option<BadField> = None;
        for (i, field) in BAD_FIELDS.iter().enumerate() {
            if mask & (1 << i) != 0 {
                apply_bad(&env, &mut p, *field);
                if first.is_none() {
                    first = Some(*field);
                }
            }
        }

        let result = client.try_register_callback(&p);
        match first {
            None => {
                // No invalid field: must be accepted.
                assert!(
                    result.is_ok(),
                    "mask {mask}: expected accept, got {result:?}"
                );
            }
            Some(f) => {
                // At least one invalid field: must be rejected with the
                // cheapest-first error for the earliest invalid field.
                assert_eq!(
                    result,
                    Err(Ok(expected_error(f))),
                    "mask {mask}: expected first error for {f:?}"
                );
            }
        }
    }
}
