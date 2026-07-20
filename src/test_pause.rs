//! Tests for the emergency pause / circuit breaker (issue #25).
//!
//! Covers the two acceptance criteria from the issue:
//! 1. Pausing blocks `register_callback` (returns `ContractPaused`) while
//!    read-only `get_transaction` on an existing record still succeeds.
//! 2. Only the admin may `pause` / `unpause`.
//!
//! Plus supporting behaviour: unpause round-trip and idempotency of pausing.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, String,
};

use crate::types::{CallbackPayload, CallbackType, ContractError, TransactionStatus};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Register + initialise the contract with all auths mocked (happy-path setup).
fn setup() -> (Env, SynapseCoreContractClient<'static>, Address, Address) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    (env, client, admin, relay)
}

/// A valid 56-character Stellar `G…` address (built from bytes so the length is
/// guaranteed correct).
fn g_address(env: &Env) -> String {
    let mut bytes = [b'A'; 56];
    bytes[0] = b'G';
    let s = core::str::from_utf8(&bytes).unwrap();
    String::from_str(env, s)
}

fn valid_payload(env: &Env) -> CallbackPayload {
    let account = g_address(env);
    CallbackPayload {
        transaction_id: String::from_str(env, "tx-1"),
        stellar_account: account.clone(),
        amount: 1_000,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: account,
        idempotency_key: String::from_str(env, "idem-1"),
        anchor_transaction_id: String::from_str(env, "anchor-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    }
}

// ─── Acceptance #1 ────────────────────────────────────────────────────────────

#[test]
fn test_pause_blocks_ingestion_but_reads_survive() {
    let (env, client, _admin, _relay) = setup();

    // Register a transaction while unpaused so a record exists to read back.
    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);
    assert_eq!(client.get_transaction(&tx_id).status, TransactionStatus::Pending);

    // Engage the circuit breaker.
    client.pause();
    assert!(client.is_paused());

    // New ingestion is rejected outright.
    let result = client.try_register_callback(&payload);
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));

    // Read-only queries remain available while paused.
    let tx = client.get_transaction(&tx_id);
    assert_eq!(tx.id, tx_id);
    assert_eq!(tx.status, TransactionStatus::Pending);
    assert!(client.health());
}

// ─── Acceptance #2 ────────────────────────────────────────────────────────────

#[test]
fn test_only_admin_can_pause_and_unpause() {
    // Build without mock_all_auths so we control exactly whose auth is present.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    client.initialize(&admin, &relay);

    let attacker = Address::generate(&env);

    // A non-admin cannot pause: only the attacker's auth is supplied, but the
    // contract requires the admin's.
    let attacker_attempt = client
        .mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "pause",
                args: ().into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_pause();
    assert!(attacker_attempt.is_err());
    assert!(!client.is_paused());

    // The admin can pause.
    client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "pause",
                args: ().into_val(&env),
                sub_invokes: &[],
            },
        }])
        .pause();
    assert!(client.is_paused());

    // A non-admin cannot unpause either.
    let attacker_unpause = client
        .mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "unpause",
                args: ().into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_unpause();
    assert!(attacker_unpause.is_err());
    assert!(client.is_paused());
}

// ─── Supporting behaviour ─────────────────────────────────────────────────────

#[test]
fn test_unpause_resumes_ingestion() {
    let (env, client, _admin, _relay) = setup();
    let payload = valid_payload(&env);

    client.pause();
    assert_eq!(
        client.try_register_callback(&payload),
        Err(Ok(ContractError::ContractPaused))
    );

    client.unpause();
    assert!(!client.is_paused());

    // Ingestion works again after unpausing.
    let tx_id = client.register_callback(&payload);
    assert_eq!(client.get_transaction(&tx_id).status, TransactionStatus::Pending);
}

#[test]
fn test_pause_is_idempotent() {
    let (_env, client, _admin, _relay) = setup();

    assert!(!client.is_paused());
    client.pause();
    client.pause(); // pausing an already-paused contract is a no-op success
    assert!(client.is_paused());

    client.unpause();
    client.unpause(); // likewise for unpause
    assert!(!client.is_paused());
}

#[test]
fn test_fresh_contract_starts_unpaused() {
    let (_env, client, _admin, _relay) = setup();
    assert!(!client.is_paused());
}
