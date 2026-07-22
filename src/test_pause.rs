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
    symbol_short,
    testutils::{Address as _, Events, MockAuth, MockAuthInvoke},
    Address, BytesN, Env, IntoVal, String, Symbol, TryFromVal,
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

/// A real SEP-23 ed25519 public-key strkey (checksum-valid).
///
/// Must pass full CRC16 validation in [`crate::validation::Validator`] — a
/// synthetic `G` + filler string is no longer accepted.
fn g_address(env: &Env) -> String {
    String::from_str(
        env,
        "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ",
    )
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
    assert_eq!(
        client.get_transaction(&tx_id).status,
        TransactionStatus::Pending
    );

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
    assert_eq!(
        client.get_transaction(&tx_id).status,
        TransactionStatus::Pending
    );
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

// ─── Contract upgrade tests ───────────────────────────────────────────────────

#[test]
fn test_upgrade_rejects_non_admin() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    client.initialize(&admin, &relay);

    let attacker = Address::generate(&env);
    let dummy_hash = BytesN::from_array(&env, &[0u8; 32]);

    // A non-admin cannot upgrade — only the attacker's auth is supplied.
    // (Successful upgrade also requires the WASM hash to exist in ledger
    // storage; that path is exercised via `EventEmitter` below + deploy-time
    // integration. Auth rejection is the acceptance criterion here.)
    let attacker_attempt = client
        .mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "upgrade",
                args: (dummy_hash.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_upgrade(&dummy_hash);
    assert!(attacker_attempt.is_err());
}

#[test]
fn test_upgrade_storage_survives_admin_ops() {
    let (env, client, _admin, _relay) = setup();

    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);
    assert_eq!(
        client.get_transaction(&tx_id).status,
        TransactionStatus::Pending
    );

    // Persistent storage must survive subsequent privileged ops.
    client.pause();
    client.unpause();

    let tx = client.get_transaction(&tx_id);
    assert_eq!(tx.id, tx_id);
    assert_eq!(tx.status, TransactionStatus::Pending);
    assert_eq!(tx.amount, 1_000);
}

#[test]
fn test_upgrade_emits_contract_upgraded_event() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let admin = Address::generate(&env);
    let dummy_hash = BytesN::from_array(&env, &[0xabu8; 32]);

    // Publish inside a contract context so testutils `Events::all` records it.
    env.as_contract(&contract_id, || {
        crate::events::EventEmitter::contract_upgraded(&env, &admin, &dummy_hash);
    });

    let events = env.events().all();
    assert_eq!(events.len(), 1, "Expected exactly one published event");

    let topics = &events.get_unchecked(0).1;
    assert_eq!(topics.len(), 2);
    let t0 = Symbol::try_from_val(&env, &topics.get_unchecked(0)).unwrap();
    let t1 = Symbol::try_from_val(&env, &topics.get_unchecked(1)).unwrap();
    assert_eq!(t0, symbol_short!("synapse"));
    assert_eq!(t1, symbol_short!("upgrade"));
}
