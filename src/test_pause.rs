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
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, BytesN, Env, IntoVal, String, Vec,
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

// ─── Contract upgrade tests ───────────────────────────────────────────────────

#[test]
fn test_upgrade_only_admin_can_upgrade() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    client.initialize(&admin, &relay);

    let attacker = Address::generate(&env);
    let dummy_hash = BytesN::from_array(&env, &[0u8; 32]);

    // A non-admin cannot upgrade — only the attacker's auth is supplied.
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

    // The admin can upgrade.
    client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "upgrade",
                args: (dummy_hash.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .upgrade(&dummy_hash);
}

#[test]
fn test_upgrade_storage_survives_same_schema_upgrade() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);

    // Register a transaction so we have data in persistent storage.
    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);
    assert_eq!(
        client.get_transaction(&tx_id).status,
        TransactionStatus::Pending
    );

    // Simulate an upgrade to the current contract's own WASM — this is a
    // no-op in practice but proves the upgrade call succeeds and that
    // persistent storage (transactions, admin, relay) survive the call.
    let current_wasm_hash = env.deployer().current_wasm_hash(&contract_id);
    client.upgrade(&current_wasm_hash);

    // After the upgrade, the transaction record still exists and is readable.
    let tx = client.get_transaction(&tx_id);
    assert_eq!(tx.id, tx_id);
    assert_eq!(tx.status, TransactionStatus::Pending);
    assert_eq!(tx.amount, 1_000);

    // Admin privileges survive: relay signer can still be rotated.
    let new_relay = Address::generate(&env);
    client.set_relay_signer(&new_relay);
}

#[test]
fn test_upgrade_emits_contract_upgraded_event() {
    let (env, client, _admin, _relay) = setup();
    let dummy_hash = BytesN::from_array(&env, &[0xabu8; 32]);
    client.upgrade(&dummy_hash);

    // Verify the upgrade event was emitted.
    let events = env.events().all();
    let upgrade_events: Vec<_> = events
        .iter()
        .filter(|e| {
            e.0.as_ref().map_or(false, |topics| {
                topics.len() == 2
                    && topics.get(0) == Some(soroban_sdk::Val::from_symbol(symbol_short!("synapse")))
                    && topics.get(1)
                        == Some(soroban_sdk::Val::from_symbol(symbol_short!("upgrade")))
            })
        })
        .collect();
    assert_eq!(upgrade_events.len(), 1, "Expected exactly one upgrade event");
}
