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
