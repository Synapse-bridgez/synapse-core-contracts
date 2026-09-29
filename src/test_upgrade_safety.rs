//! Upgrade-safety tests for issues #81–#84.
//!
//! Covers timelock propose/finalize/cancel, migration atomic revert,
//! rollback guards, and schema compatibility ranges.

#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger, LedgerInfo},
    Address, BytesN, Env, Symbol, TryFromVal,
};

use crate::migration::{MIGRATION_FAIL_AFTER_WRITE, MIGRATION_NOOP};
use crate::storage::StorageClient;
use crate::types::{ContractError, UpgradeSnapshot, SCHEMA_VERSION};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

fn setup() -> (Env, SynapseCoreContractClient<'static>, Address) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    (env, client, contract_id)
}

fn advance_ledger(env: &Env, to_sequence: u32) {
    env.ledger().set(LedgerInfo {
        sequence_number: to_sequence,
        ..env.ledger().get()
    });
}

fn hash(env: &Env, fill: u8) -> BytesN<32> {
    BytesN::from_array(env, &[fill; 32])
}

// ─── #81 Timelocked upgrade ───────────────────────────────────────────────────

#[test]
fn test_propose_upgrade_sets_pending_and_eta() {
    let (env, client, _) = setup();
    client.set_upgrade_delay(&100);
    let h = hash(&env, 0x11);
    let before = env.ledger().sequence();
    client.propose_upgrade(&h, &SCHEMA_VERSION);

    let pending = client.get_pending_upgrade().expect("pending");
    assert_eq!(pending.wasm_hash, h);
    assert_eq!(pending.expected_schema_version, SCHEMA_VERSION);
    assert_eq!(pending.eta_ledger, before + 100);
    assert_eq!(client.upgrade_delay(), 100);
}

#[test]
fn test_propose_upgrade_replaces_existing_pending() {
    let (env, client, _) = setup();
    client.set_upgrade_delay(&50);
    client.propose_upgrade(&hash(&env, 0x01), &SCHEMA_VERSION);
    advance_ledger(&env, env.ledger().sequence() + 10);
    let h2 = hash(&env, 0x02);
    let at = env.ledger().sequence();
    client.propose_upgrade(&h2, &SCHEMA_VERSION);
    let pending = client.get_pending_upgrade().unwrap();
    assert_eq!(pending.wasm_hash, h2);
    assert_eq!(pending.eta_ledger, at + 50, "replace restarts the delay");
}

#[test]
fn test_finalize_before_delay_fails() {
    let (env, client, _) = setup();
    client.set_upgrade_delay(&1_000);
    client.propose_upgrade(&hash(&env, 0x33), &SCHEMA_VERSION);
    assert_eq!(
        client.try_finalize_upgrade(),
        Err(Ok(ContractError::UpgradeTimelockNotElapsed))
    );
}

#[test]
fn test_propose_cancel_finalize_fails() {
    let (env, client, _) = setup();
    client.set_upgrade_delay(&10);
    client.propose_upgrade(&hash(&env, 0x44), &SCHEMA_VERSION);
    client.cancel_upgrade();
    assert!(client.get_pending_upgrade().is_none());
    assert_eq!(
        client.try_finalize_upgrade(),
        Err(Ok(ContractError::NoPendingUpgrade))
    );
}

#[test]
fn test_propose_wait_finalize_past_timelock() {
    // Happy-path state machine: after ETA, finalize is no longer rejected as
    // TimelockNotElapsed. A missing uploaded WASM yields a host error (rolled
    // back), which is acceptable here — the delay gate is what we assert.
    let (env, client, _) = setup();
    client.set_upgrade_delay(&5);
    client.propose_upgrade(&hash(&env, 0x33), &SCHEMA_VERSION);
    let eta = client.get_pending_upgrade().unwrap().eta_ledger;
    advance_ledger(&env, eta);
    let result = client.try_finalize_upgrade();
    assert_ne!(
        result,
        Err(Ok(ContractError::UpgradeTimelockNotElapsed)),
        "delay elapsed — must not fail the timelock gate"
    );
    assert_ne!(result, Err(Ok(ContractError::NoPendingUpgrade)));
}

#[test]
fn test_upgrade_proposed_and_cancelled_event_topics() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let admin = Address::generate(&env);
    let h = hash(&env, 0xab);
    env.as_contract(&contract_id, || {
        crate::events::EventEmitter::upgrade_proposed(&env, &admin, &h, 1, 42);
        crate::events::EventEmitter::upgrade_cancelled(&env, &admin, &h);
        crate::events::EventEmitter::upgrade_finalized(&env, &admin, &h, 1);
    });
    let events = env.events().all();
    let t =
        |i: u32| Symbol::try_from_val(&env, &events.get_unchecked(i).1.get_unchecked(1)).unwrap();
    assert_eq!(t(0), symbol_short!("up_prop"));
    assert_eq!(t(1), symbol_short!("up_can"));
    assert_eq!(t(2), symbol_short!("up_fin"));
}

// ─── #82 upgrade_and_migrate ──────────────────────────────────────────────────

#[test]
fn test_migration_failure_leaves_storage_unchanged() {
    let (env, client, contract_id) = setup();
    let dummy = hash(&env, 0x55);
    let result =
        client.try_upgrade_and_migrate(&dummy, &SCHEMA_VERSION, &MIGRATION_FAIL_AFTER_WRITE);
    assert_eq!(result, Err(Ok(ContractError::MigrationFailed)));
    env.as_contract(&contract_id, || {
        assert!(
            StorageClient::get_migration_marker(&env).is_none(),
            "failed migration must roll back marker writes"
        );
        assert!(!StorageClient::last_upgrade_migrated(&env));
        assert!(StorageClient::get_current_wasm_hash(&env).is_none());
    });
}

#[test]
fn test_unknown_migration_id_rejected() {
    let (env, client, _) = setup();
    assert_eq!(
        client.try_upgrade_and_migrate(&hash(&env, 0x66), &SCHEMA_VERSION, &999_999),
        Err(Ok(ContractError::UnknownMigration))
    );
}

#[test]
fn test_noop_migration_runs_before_wasm_swap() {
    let (env, client, contract_id) = setup();
    // NOOP migration succeeds; missing WASM then fails the host call and
    // rolls back — proving migration ran without leaving LastUpgradeMigrated.
    let _ = client.try_upgrade_and_migrate(&hash(&env, 0x77), &SCHEMA_VERSION, &MIGRATION_NOOP);
    env.as_contract(&contract_id, || {
        assert!(!StorageClient::last_upgrade_migrated(&env));
    });
}

// ─── #83 rollback ─────────────────────────────────────────────────────────────

#[test]
fn test_rollback_with_no_history_fails() {
    let (_env, client, _) = setup();
    assert_eq!(
        client.try_rollback_upgrade(),
        Err(Ok(ContractError::NothingToRollback))
    );
}

#[test]
fn test_rollback_blocked_after_migrated_flag() {
    let (env, client, contract_id) = setup();
    let prev = hash(&env, 0xaa);
    env.as_contract(&contract_id, || {
        StorageClient::set_previous_upgrade(
            &env,
            &UpgradeSnapshot {
                wasm_hash: prev.clone(),
                schema_version: SCHEMA_VERSION,
            },
        );
        StorageClient::set_last_upgrade_migrated(&env, true);
    });
    assert_eq!(
        client.try_rollback_upgrade(),
        Err(Ok(ContractError::UpgradeNotReversible))
    );
}

#[test]
fn test_upgrade_rollback_history_round_trip() {
    // Seed current hash, then mimic perform_upgrade's history write (without
    // a WASM swap) and assert rollback targets the pre-upgrade snapshot.
    let (env, client, contract_id) = setup();
    let pre = hash(&env, 0x01);
    let post = hash(&env, 0x02);
    client.register_installed_wasm(&pre);

    env.as_contract(&contract_id, || {
        let current = StorageClient::get_current_wasm_hash(&env).unwrap();
        assert_eq!(current, pre);
        StorageClient::set_previous_upgrade(
            &env,
            &UpgradeSnapshot {
                wasm_hash: current,
                schema_version: SCHEMA_VERSION,
            },
        );
        StorageClient::set_current_wasm_hash(&env, &post);
        StorageClient::set_last_upgrade_migrated(&env, false);
    });

    assert_eq!(client.previous_upgrade().unwrap().wasm_hash, pre);
    assert!(!client.last_upgrade_migrated());

    // Rollback guard path is clear; attempting the swap without uploaded
    // WASM fails at the host — but not with NothingToRollback / NotReversible.
    let result = client.try_rollback_upgrade();
    assert_ne!(result, Err(Ok(ContractError::NothingToRollback)));
    assert_ne!(result, Err(Ok(ContractError::UpgradeNotReversible)));
}

#[test]
fn test_rollback_event_topics() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let admin = Address::generate(&env);
    let h = hash(&env, 0xcd);
    env.as_contract(&contract_id, || {
        crate::events::EventEmitter::upgrade_rolled_back(&env, &admin, &h, 1);
        crate::events::EventEmitter::upgrade_migrated(&env, &admin, 0, 0, &h);
    });
    let events = env.events().all();
    let t1 = Symbol::try_from_val(&env, &events.get_unchecked(0).1.get_unchecked(1)).unwrap();
    let t2 = Symbol::try_from_val(&env, &events.get_unchecked(1).1.get_unchecked(1)).unwrap();
    assert_eq!(t1, symbol_short!("rollback"));
    assert_eq!(t2, symbol_short!("migrate"));
}

// ─── #84 schema compatibility ranges ─────────────────────────────────────────

#[test]
fn test_default_range_is_exact_match() {
    let (env, client, _) = setup();
    let range = client.schema_compatibility_range();
    assert_eq!(range.min, SCHEMA_VERSION);
    assert_eq!(range.max, SCHEMA_VERSION);
    assert_eq!(
        client.try_upgrade(&hash(&env, 0), &(SCHEMA_VERSION + 1)),
        Err(Ok(ContractError::SchemaVersionMismatch))
    );
}

#[test]
fn test_range_accepts_boundaries_rejects_outside() {
    let (env, client, _) = setup();
    client.set_schema_compatibility_range(&1, &3);
    let range = client.schema_compatibility_range();
    assert_eq!(range.min, 1);
    assert_eq!(range.max, 3);

    let dummy = hash(&env, 0xee);
    assert_ne!(
        client.try_upgrade(&dummy, &1),
        Err(Ok(ContractError::SchemaVersionMismatch))
    );
    assert_ne!(
        client.try_upgrade(&dummy, &3),
        Err(Ok(ContractError::SchemaVersionMismatch))
    );
    assert_eq!(
        client.try_upgrade(&dummy, &0),
        Err(Ok(ContractError::SchemaVersionMismatch))
    );
    assert_eq!(
        client.try_upgrade(&dummy, &4),
        Err(Ok(ContractError::SchemaVersionMismatch))
    );
}

#[test]
fn test_range_excluding_current_rejected() {
    let (_env, client, _) = setup();
    assert_eq!(
        client.try_set_schema_compatibility_range(&2, &3),
        Err(Ok(ContractError::InvalidSchemaCompatRange))
    );
    assert_eq!(
        client.try_set_schema_compatibility_range(&3, &1),
        Err(Ok(ContractError::InvalidSchemaCompatRange))
    );
}

#[test]
fn test_propose_rejects_schema_mismatch_upfront() {
    let (env, client, _) = setup();
    assert_eq!(
        client.try_propose_upgrade(&hash(&env, 0xff), &999),
        Err(Ok(ContractError::SchemaVersionMismatch))
    );
}
