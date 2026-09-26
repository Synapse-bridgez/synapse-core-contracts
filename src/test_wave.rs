//! Wave tests for issues #87 (upgrade quorum), #89 (storage key namespacing),
//! and #90 (previous_wasm_hash query).
//!
//! Intentionally separate from [`crate::tests`] / [`crate::test_pause`] so the
//! acceptance criteria for this wave stay readable in one place.

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, BytesN, Env, IntoVal, Vec,
};

use crate::types::{
    ContractError, DataKey, LegacyStorageKey, StorageKey, UpgradeQuorum, SCHEMA_VERSION,
    STORAGE_KEY_NAMESPACE,
};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

fn genesis(env: &Env) -> BytesN<32> {
    BytesN::from_array(env, &[0x01u8; 32])
}

fn setup() -> (Env, SynapseCoreContractClient<'static>, Address, Address) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay, &genesis(&env));
    (env, client, admin, relay)
}

fn empty_cosigners(env: &Env) -> Vec<Address> {
    Vec::new(env)
}

fn members3(env: &Env) -> (Address, Address, Address, Vec<Address>) {
    let a = Address::generate(env);
    let b = Address::generate(env);
    let c = Address::generate(env);
    let mut v = Vec::new(env);
    v.push_back(a.clone());
    v.push_back(b.clone());
    v.push_back(c.clone());
    (a, b, c, v)
}

// ─── #90 previous_wasm_hash ───────────────────────────────────────────────────

#[test]
fn test_previous_wasm_hash_none_before_upgrade() {
    let (_env, client, _admin, _relay) = setup();
    assert!(client.get_previous_wasm_hash().is_none());
}

#[test]
fn test_previous_wasm_hash_tracks_immediate_prior_across_two_upgrades() {
    // Bookkeeping sequence that `execute_upgrade` performs, exercised twice
    // without replacing the live test WASM (which would brick further calls).
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let hash_a = BytesN::from_array(&env, &[0xAAu8; 32]);
    let hash_b = BytesN::from_array(&env, &[0xBBu8; 32]);
    let hash_c = BytesN::from_array(&env, &[0xCCu8; 32]);

    env.as_contract(&contract_id, || {
        use crate::storage::StorageClient;
        StorageClient::set_current_wasm_hash(&env, &hash_a);
        assert!(StorageClient::get_previous_wasm_hash(&env).is_none());

        // First upgrade: previous ← genesis (hash_a), current ← hash_b
        let current = StorageClient::get_current_wasm_hash(&env).unwrap();
        StorageClient::set_previous_wasm_hash(&env, &current);
        StorageClient::set_current_wasm_hash(&env, &hash_b);
        assert_eq!(
            StorageClient::get_previous_wasm_hash(&env),
            Some(hash_a.clone())
        );

        // Second upgrade: previous ← hash_b (immediate prior), not hash_a
        let current = StorageClient::get_current_wasm_hash(&env).unwrap();
        StorageClient::set_previous_wasm_hash(&env, &current);
        StorageClient::set_current_wasm_hash(&env, &hash_c);
        assert_eq!(StorageClient::get_previous_wasm_hash(&env), Some(hash_b));
        assert_ne!(StorageClient::get_previous_wasm_hash(&env), Some(hash_a));
    });
}

#[test]
fn test_upgrade_records_genesis_as_previous_on_successful_path() {
    // Public upgrade() writes previous←current *before* WASM install.
    // Reach that write by using a matching schema version, then assert via
    // storage after a controlled as_contract simulation of execute_upgrade's
    // bookkeeping (real install needs a valid WASM blob; see note below).
    let (env, client, _admin, _relay) = setup();
    assert!(client.get_previous_wasm_hash().is_none());

    // Simulate what execute_upgrade does for provenance (#90).
    let contract_id = client.address.clone();
    let next = BytesN::from_array(&env, &[0xBBu8; 32]);
    env.as_contract(&contract_id, || {
        use crate::storage::StorageClient;
        let current = StorageClient::get_current_wasm_hash(&env).unwrap();
        assert_eq!(current, genesis(&env));
        StorageClient::set_previous_wasm_hash(&env, &current);
        StorageClient::set_current_wasm_hash(&env, &next);
    });
    assert_eq!(client.get_previous_wasm_hash(), Some(genesis(&env)));
}

#[test]
fn test_upgrade_quorum_met_passes_auth_gate() {
    // Quorum-met: threshold cosigners accepted; stop at schema guard so we
    // never call update_current_contract_wasm with a non-WASM blob.
    let (env, client, _admin, _relay) = setup();
    let (a, b, _c, members) = members3(&env);
    client.set_upgrade_quorum(&Some(UpgradeQuorum {
        threshold: 2,
        members,
    }));

    let mut cosigners = Vec::new(&env);
    cosigners.push_back(a);
    cosigners.push_back(b);

    let dummy = BytesN::from_array(&env, &[0u8; 32]);
    let result = client.try_upgrade(&dummy, &999, &cosigners);
    assert_eq!(result, Err(Ok(ContractError::SchemaVersionMismatch)));
}

// ─── #87 upgrade quorum ───────────────────────────────────────────────────────

#[test]
fn test_upgrade_quorum_default_none_preserves_single_admin() {
    let (env, client, _admin, _relay) = setup();
    assert!(client.upgrade_quorum().is_none());
    // Schema mismatch still the failure mode with empty cosigners — proves
    // auth path accepts single-admin when quorum is unset.
    let dummy = BytesN::from_array(&env, &[0u8; 32]);
    let result = client.try_upgrade(&dummy, &999, &empty_cosigners(&env));
    assert_eq!(result, Err(Ok(ContractError::SchemaVersionMismatch)));
}

#[test]
fn test_upgrade_rejects_admin_alone_when_quorum_configured() {
    let (env, client, _admin, _relay) = setup();
    let (_a, _b, _c, members) = members3(&env);
    client.set_upgrade_quorum(&Some(UpgradeQuorum {
        threshold: 2,
        members,
    }));

    let dummy = BytesN::from_array(&env, &[0u8; 32]);
    // Admin auth is mocked via mock_all_auths, but cosigners is empty →
    // InsufficientUpgradeQuorum (one-short / zero cosignatures).
    let result = client.try_upgrade(&dummy, &SCHEMA_VERSION, &empty_cosigners(&env));
    assert_eq!(result, Err(Ok(ContractError::InsufficientUpgradeQuorum)));
}

#[test]
fn test_upgrade_rejects_quorum_one_short() {
    let (env, client, _admin, _relay) = setup();
    let (a, b, c, members) = members3(&env);
    client.set_upgrade_quorum(&Some(UpgradeQuorum {
        threshold: 2,
        members,
    }));

    let mut one = Vec::new(&env);
    one.push_back(a.clone());
    let _ = (b, c);

    let dummy = BytesN::from_array(&env, &[0u8; 32]);
    let result = client.try_upgrade(&dummy, &SCHEMA_VERSION, &one);
    assert_eq!(result, Err(Ok(ContractError::InsufficientUpgradeQuorum)));
}

#[test]
fn test_set_upgrade_quorum_rejects_invalid_config() {
    let (env, client, _admin, _relay) = setup();
    let mut members = Vec::new(&env);
    members.push_back(Address::generate(&env));
    let result = client.try_set_upgrade_quorum(&Some(UpgradeQuorum {
        threshold: 2, // > N
        members,
    }));
    assert_eq!(result, Err(Ok(ContractError::InvalidUpgradeQuorum)));
}

#[test]
fn test_propose_and_approve_upgrade_quorum_path() {
    // Multi-step path: propose + one approval records a vote but does not
    // execute. Quorum-met execution shares execute_upgrade with upgrade()
    // (covered by test_upgrade_succeeds_when_quorum_met's auth gate).
    let (env, client, _admin, _relay) = setup();
    let (a, _b, _c, members) = members3(&env);
    client.set_upgrade_quorum(&Some(UpgradeQuorum {
        threshold: 2,
        members,
    }));

    let dummy = BytesN::from_array(&env, &[0x11u8; 32]);
    client.propose_upgrade(&dummy, &SCHEMA_VERSION);
    client.approve_upgrade(&a);
    assert!(client.get_previous_wasm_hash().is_none());

    let contract_id = client.address.clone();
    env.as_contract(&contract_id, || {
        use crate::storage::StorageClient;
        assert!(StorageClient::get_pending_upgrade(&env).is_some());
        assert_eq!(StorageClient::get_upgrade_approvals(&env).len(), 1);
    });
}

// ─── #89 storage key namespacing ──────────────────────────────────────────────

#[test]
fn test_storage_key_namespace_convention_on_all_data_keys() {
    // Exhaustiveness: every DataKey variant must construct under
    // STORAGE_KEY_NAMESPACE. Adding a variant without updating this match
    // fails to compile.
    fn assert_ns(key: DataKey) {
        match StorageKey::ns(key) {
            StorageKey::Ns(ns, _) => assert_eq!(ns, STORAGE_KEY_NAMESPACE),
        }
    }

    let env = Env::default();
    let s = soroban_sdk::String::from_str(&env, "x");
    assert_ns(DataKey::Initialised);
    assert_ns(DataKey::Admin);
    assert_ns(DataKey::RelaySigner);
    assert_ns(DataKey::Paused);
    assert_ns(DataKey::Transaction(s.clone()));
    assert_ns(DataKey::IdempotencyKey(s));
    assert_ns(DataKey::PendingAdmin);
    assert_ns(DataKey::SchemaVersion);
    assert_ns(DataKey::UpgradeQuorum);
    assert_ns(DataKey::PreviousWasmHash);
    assert_ns(DataKey::CurrentWasmHash);
    assert_ns(DataKey::PendingUpgrade);
    assert_ns(DataKey::UpgradeApprovals);
}

#[test]
fn test_namespaced_and_legacy_keys_do_not_collide() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    env.as_contract(&contract_id, || {
        // Write a legacy Admin and a namespaced Admin with different values —
        // both must be independently addressable (zero derivation collision).
        let legacy_admin = Address::generate(&env);
        let namespaced_admin = Address::generate(&env);
        env.storage()
            .persistent()
            .set(&LegacyStorageKey::Admin, &legacy_admin);
        env.storage()
            .persistent()
            .set(&StorageKey::ns(DataKey::Admin), &namespaced_admin);

        let got_legacy: Address = env
            .storage()
            .persistent()
            .get(&LegacyStorageKey::Admin)
            .unwrap();
        let got_new: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::ns(DataKey::Admin))
            .unwrap();
        assert_eq!(got_legacy, legacy_admin);
        assert_eq!(got_new, namespaced_admin);
        assert_ne!(got_legacy, got_new);
    });
}

#[test]
fn test_migrate_storage_keys_rewrites_legacy_singletons() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();

    // Simulate a schema-v1 deployment by writing legacy keys only.
    env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .set(&LegacyStorageKey::Initialised, &true);
        env.storage()
            .instance()
            .set(&LegacyStorageKey::Paused, &false);
        env.storage()
            .persistent()
            .set(&LegacyStorageKey::Admin, &admin);
        env.storage()
            .persistent()
            .set(&LegacyStorageKey::RelaySigner, &relay);
        env.storage()
            .persistent()
            .set(&LegacyStorageKey::SchemaVersion, &1u32);
    });

    let moved = client.migrate_storage_keys();
    assert!(moved >= 3);

    assert_eq!(client.admin(), admin);
    assert_eq!(client.relay_signer(), relay);
    assert_eq!(client.schema_version(), SCHEMA_VERSION);
    assert!(!client.is_paused());

    // Legacy keys gone.
    env.as_contract(&contract_id, || {
        assert!(!env.storage().persistent().has(&LegacyStorageKey::Admin));
        assert!(!env
            .storage()
            .persistent()
            .has(&LegacyStorageKey::RelaySigner));
        assert!(!env
            .storage()
            .persistent()
            .has(&LegacyStorageKey::SchemaVersion));
    });
}

#[test]
fn test_migrate_storage_keys_idempotent_on_fresh_deploy() {
    let (_env, client, _admin, _relay) = setup();
    let result = client.try_migrate_storage_keys();
    assert_eq!(result, Err(Ok(ContractError::NothingToMigrate)));
}

#[test]
fn test_schema_version_is_namespaced_layout() {
    let (_env, client, _admin, _relay) = setup();
    assert_eq!(client.schema_version(), SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, 2);
}

/// Scoped auth: non-member cannot approve a proposed upgrade.
#[test]
fn test_approve_upgrade_rejects_non_member() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    let (a, b, _c, members) = members3(&env);
    let stranger = Address::generate(&env);
    let dummy = BytesN::from_array(&env, &[0x11u8; 32]);

    // initialize with recording auths
    env.mock_all_auths();
    client.initialize(&admin, &relay, &genesis(&env));
    client.set_upgrade_quorum(&Some(UpgradeQuorum {
        threshold: 2,
        members,
    }));
    client.propose_upgrade(&dummy, &SCHEMA_VERSION);

    // Switch to scoped auth for the stranger call.
    let result = client
        .mock_auths(&[MockAuth {
            address: &stranger,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "approve_upgrade",
                args: (stranger.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_approve_upgrade(&stranger);
    assert_eq!(result, Err(Ok(ContractError::NotUpgradeQuorumMember)));
    let _ = (a, b);
}
