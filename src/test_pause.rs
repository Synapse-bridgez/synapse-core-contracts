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
    let genesis = BytesN::from_array(&env, &[0x01u8; 32]);
    client.initialize(&admin, &relay, &genesis);
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
    let genesis = BytesN::from_array(&env, &[0x01u8; 32]);
    client.initialize(&admin, &relay, &genesis);

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
    let genesis = BytesN::from_array(&env, &[0x01u8; 32]);
    client.initialize(&admin, &relay, &genesis);

    let attacker = Address::generate(&env);
    let dummy_hash = BytesN::from_array(&env, &[0u8; 32]);

    // A non-admin cannot upgrade — only the attacker's auth is supplied.
    // Successful upgrade also requires the WASM hash to exist in ledger storage;
    // auth rejection is the acceptance criterion here. Topic assertions for the
    // upgrade event live in `test_upgrade_emits_contract_upgraded_event` and
    // are cross-checked against EVENTS.md.
    // (Successful upgrade also requires the WASM hash to exist in ledger
    // storage; that path is exercised via `EventEmitter` below + deploy-time
    // integration. Auth rejection is the acceptance criterion here.)
    let attacker_attempt = client
        .mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "upgrade",
                args: (dummy_hash.clone(), 1u32).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_upgrade(&dummy_hash, &1);
    assert!(attacker_attempt.is_err());
}

#[test]
fn test_upgrade_rejects_schema_version_mismatch() {
    // F-04: the mismatch check runs before update_current_contract_wasm, so
    // this is testable without a real uploaded WASM hash.
    let (env, client, _admin, _relay) = setup();
    let dummy_hash = BytesN::from_array(&env, &[0u8; 32]);
    let result = client.try_upgrade(&dummy_hash, &999);
    assert_eq!(result, Err(Ok(ContractError::SchemaVersionMismatch)));
}

#[test]
#[ignore = "entry point is a stub until #199 restores it"]
fn test_schema_version_query_returns_current_version() {
    let (_env, client, _admin, _relay) = setup();
    assert_eq!(client.schema_version(), 2);
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
    // Topics must match EVENTS.md: synapse / upgrade.
    env.as_contract(&contract_id, || {
        crate::events::EventEmitter::contract_upgraded(&env, &admin, &dummy_hash, 1);
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

// Minimal valid WASM used only to satisfy the host's "hash must exist in
// ledger" check during upgrade tests. Sourced from soroban-sdk doctest fixtures.
const MINIMAL_WASM: &[u8] = include_bytes!("../testdata/minimal.wasm");

fn upload_minimal(env: &Env) -> BytesN<32> {
    env.deployer().upload_contract_wasm(MINIMAL_WASM)
}

// ─── #80 post-upgrade self-check ─────────────────────────────────────────────

#[test]
fn test_post_upgrade_self_check_passes_on_healthy_state() {
    let (env, client, _admin, _relay) = setup();
    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        crate::storage::StorageClient::post_upgrade_self_check(&env).expect("healthy");
    });
}

#[test]
#[ignore = "entry point is a stub until #199 restores it"]
fn test_post_upgrade_self_check_fails_when_relay_missing() {
    let (env, client, admin, _relay) = setup();
    let contract_id = client.address.clone();

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .remove(&crate::types::StorageKey::RelaySigner);
        let err = crate::storage::StorageClient::post_upgrade_self_check(&env).unwrap_err();
        assert_eq!(err, ContractError::SelfCheckFailed);
    });

    // Admin / schema still readable — only relay was corrupted.
    assert_eq!(client.admin(), admin);
    assert_eq!(client.schema_version(), 1);
}

#[test]
#[ignore = "entry point is a stub until #199 restores it"]
fn test_upgrade_self_check_failure_reverts_without_history() {
    // Corrupted core storage + a ledger-resident WASM hash: upgrade passes
    // auth/schema guards, requests the WASM swap, then self-check fails.
    // Returning Err rolls back the invocation (host defers the code swap until
    // success), so history must stay empty and trust-root keys unchanged.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay, &BytesN::from_array(&env, &[0x01u8; 32]));

    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .remove(&crate::types::StorageKey::RelaySigner);
    });

    let wasm_hash = upload_minimal(&env);
    let pre_admin = client.admin();
    let pre_schema = client.schema_version();
    assert!(client.get_upgrade_history().is_empty());

    let result = client.try_upgrade(&wasm_hash, &1);
    assert_eq!(result, Err(Ok(ContractError::SelfCheckFailed)));

    // Post-state identical for everything the failed upgrade would have written.
    assert_eq!(client.admin(), pre_admin);
    assert_eq!(client.schema_version(), pre_schema);
    assert!(client.get_upgrade_history().is_empty());
    assert!(client.health());
}

#[test]
fn test_self_check_events_topics() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    env.as_contract(&contract_id, || {
        crate::events::EventEmitter::upgrade_self_check_passed(&env, 1);
        crate::events::EventEmitter::upgrade_self_check_failed(&env, 1);
    });
    let events = env.events().all();
    assert_eq!(events.len(), 2);
    let t_pass = Symbol::try_from_val(&env, &events.get_unchecked(0).1.get_unchecked(1)).unwrap();
    let t_fail = Symbol::try_from_val(&env, &events.get_unchecked(1).1.get_unchecked(1)).unwrap();
    assert_eq!(t_pass, symbol_short!("chk_pass"));
    assert_eq!(t_fail, symbol_short!("chk_fail"));
}

// ─── #85 simulate_upgrade ────────────────────────────────────────────────────

#[test]
fn test_simulate_upgrade_compatible_and_schema_mismatch() {
    let (env, client, admin, _relay) = setup();
    let hash = BytesN::from_array(&env, &[1u8; 32]);

    assert_eq!(
        client.simulate_upgrade(&admin, &hash, &1),
        crate::types::UpgradeCompatibility::Compatible
    );
    assert_eq!(
        client.simulate_upgrade(&admin, &hash, &999),
        crate::types::UpgradeCompatibility::SchemaVersionMismatch
    );
}

#[test]
fn test_simulate_upgrade_caller_not_admin() {
    let (env, client, _admin, _relay) = setup();
    let stranger = Address::generate(&env);
    let hash = BytesN::from_array(&env, &[1u8; 32]);
    assert_eq!(
        client.simulate_upgrade(&stranger, &hash, &1),
        crate::types::UpgradeCompatibility::CallerNotAdmin
    );
}

#[test]
fn test_simulate_upgrade_not_initialised() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let anyone = Address::generate(&env);
    let hash = BytesN::from_array(&env, &[1u8; 32]);
    assert_eq!(
        client.simulate_upgrade(&anyone, &hash, &1),
        crate::types::UpgradeCompatibility::NotInitialised
    );
}

#[test]
fn test_simulate_upgrade_is_side_effect_free() {
    let (env, client, admin, relay) = setup();
    let contract_id = client.address.clone();
    let hash = BytesN::from_array(&env, &[9u8; 32]);

    // Fingerprint persistent+instance keys we care about before/after.
    let before_history = client.get_upgrade_history().len();
    let before_paused = client.is_paused();
    let before_schema = client.schema_version();

    let _ = client.simulate_upgrade(&admin, &hash, &1);
    let _ = client.simulate_upgrade(&admin, &hash, &999);
    let _ = client.simulate_upgrade(&relay, &hash, &1);

    assert_eq!(client.get_upgrade_history().len(), before_history);
    assert_eq!(client.is_paused(), before_paused);
    assert_eq!(client.schema_version(), before_schema);
    assert_eq!(client.admin(), admin);
    assert_eq!(client.relay_signer(), relay);

    // No CurrentWasmHash write either.
    env.as_contract(&contract_id, || {
        assert!(!env
            .storage()
            .persistent()
            .has(&crate::types::StorageKey::CurrentWasmHash));
    });
}

#[test]
#[ignore = "entry point is a stub until #199 restores it"]
fn test_simulate_upgrade_matches_real_upgrade_guards() {
    // Property: for every distinct guard failure, simulate's verdict matches
    // what a subsequent upgrade() call returns. Compatible + missing WASM is
    // out of scope (host-level); we upload a hash for the success path.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    let stranger = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay, &BytesN::from_array(&env, &[0x01u8; 32]));

    let hash = upload_minimal(&env);

    // Schema mismatch
    assert_eq!(
        client.simulate_upgrade(&admin, &hash, &42),
        crate::types::UpgradeCompatibility::SchemaVersionMismatch
    );
    assert_eq!(
        client.try_upgrade(&hash, &42),
        Err(Ok(ContractError::SchemaVersionMismatch))
    );

    // Wrong caller — simulate distinguishes auth; real upgrade fails auth.
    assert_eq!(
        client.simulate_upgrade(&stranger, &hash, &1),
        crate::types::UpgradeCompatibility::CallerNotAdmin
    );
    let auth_fail = client
        .mock_auths(&[MockAuth {
            address: &stranger,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "upgrade",
                args: (hash.clone(), 1u32).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_upgrade(&hash, &1);
    assert!(auth_fail.is_err());

    // Compatible → real upgrade succeeds (history written; code becomes minimal).
    assert_eq!(
        client.simulate_upgrade(&admin, &hash, &1),
        crate::types::UpgradeCompatibility::Compatible
    );
    client.upgrade(&hash, &1);
    env.as_contract(&contract_id, || {
        let hist = crate::storage::StorageClient::get_upgrade_history(&env);
        assert_eq!(hist.len(), 1);
        assert_eq!(hist.get_unchecked(0).new_wasm_hash, hash);
        assert_eq!(hist.get_unchecked(0).admin, admin);
    });
}

// ─── #86 upgrade history ─────────────────────────────────────────────────────

#[test]
#[ignore = "entry point is a stub until #199 restores it"]
fn test_upgrade_history_ordered_across_multiple_appends() {
    let (env, client, admin, _relay) = setup();
    let contract_id = client.address.clone();

    assert!(client.get_upgrade_history().is_empty());

    env.as_contract(&contract_id, || {
        for i in 0u8..5 {
            let previous = BytesN::from_array(&env, &[i; 32]);
            let new_hash = BytesN::from_array(&env, &[i + 1; 32]);
            crate::storage::StorageClient::append_upgrade_record(
                &env,
                &crate::types::UpgradeRecord {
                    previous_wasm_hash: previous,
                    new_wasm_hash: new_hash,
                    schema_version: 1,
                    ledger: 100 + u32::from(i),
                    admin: admin.clone(),
                },
            );
        }
    });

    let hist = client.get_upgrade_history();
    assert_eq!(hist.len(), 5);
    for i in 0u8..5 {
        let rec = hist.get_unchecked(u32::from(i));
        assert_eq!(rec.previous_wasm_hash, BytesN::from_array(&env, &[i; 32]));
        assert_eq!(rec.new_wasm_hash, BytesN::from_array(&env, &[i + 1; 32]));
        assert_eq!(rec.ledger, 100 + u32::from(i));
        assert_eq!(rec.admin, admin);
    }
}

#[test]
#[ignore = "entry point is a stub until #199 restores it"]
fn test_upgrade_history_evicts_oldest_at_cap() {
    let (env, client, admin, _relay) = setup();
    let contract_id = client.address.clone();
    let cap = crate::types::MAX_UPGRADE_HISTORY;

    env.as_contract(&contract_id, || {
        for i in 0..cap + 3 {
            let b = (i % 256) as u8;
            crate::storage::StorageClient::append_upgrade_record(
                &env,
                &crate::types::UpgradeRecord {
                    previous_wasm_hash: BytesN::from_array(&env, &[b; 32]),
                    new_wasm_hash: BytesN::from_array(&env, &[b.wrapping_add(1); 32]),
                    schema_version: 1,
                    ledger: i,
                    admin: admin.clone(),
                },
            );
        }
    });

    let hist = client.get_upgrade_history();
    assert_eq!(hist.len(), cap);
    // Oldest surviving ledger is 3 (0,1,2 evicted).
    assert_eq!(hist.get_unchecked(0).ledger, 3);
    assert_eq!(hist.get_unchecked(cap - 1).ledger, cap + 2);
}

#[test]
#[ignore = "entry point is a stub until #199 restores it"]
fn test_upgrade_history_survives_in_persistent_tier() {
    // History uses persistent storage (same tier as admin) so it survives
    // upgrades. Verified here by writing a record then confirming it is still
    // readable after unrelated privileged ops (pause round-trip), matching the
    // existing upgrade-survival suite pattern.
    let (env, client, admin, _relay) = setup();
    let contract_id = client.address.clone();
    let hash = BytesN::from_array(&env, &[0x11u8; 32]);

    env.as_contract(&contract_id, || {
        crate::storage::StorageClient::append_upgrade_record(
            &env,
            &crate::types::UpgradeRecord {
                previous_wasm_hash: BytesN::from_array(&env, &[0u8; 32]),
                new_wasm_hash: hash.clone(),
                schema_version: 1,
                ledger: 7,
                admin: admin.clone(),
            },
        );
    });

    client.pause();
    client.unpause();

    let hist = client.get_upgrade_history();
    assert_eq!(hist.len(), 1);
    assert_eq!(hist.get_unchecked(0).new_wasm_hash, hash);
    assert_eq!(hist.get_unchecked(0).ledger, 7);
}
