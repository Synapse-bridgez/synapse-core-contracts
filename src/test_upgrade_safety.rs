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
    client.initialize(&admin, &relay, &BytesN::from_array(&env, &[0x01u8; 32]));
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
        Err(Ok(ContractError::TimelockNotElapsed))
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
        Err(Ok(ContractError::NoPendingChange))
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
        Err(Ok(ContractError::TimelockNotElapsed)),
        "delay elapsed — must not fail the timelock gate"
    );
    assert_ne!(result, Err(Ok(ContractError::NoPendingChange)));
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

// ─── #319 cross-feature guard consistency ────────────────────────────────────

#[test]
fn test_timelock_not_elapsed_boundary_consistent_across_features() {
    // Both upgrade finalization (#100) and relay-signer rotation finalization
    // (#87) must reject exactly one ledger before the configured delay and
    // accept at the ETA — identical relative boundary.
    let (env, client, _) = setup();
    let delay: u32 = 100;

    // Upgrade flow.
    client.set_upgrade_delay(&delay);
    client.propose_upgrade(&hash(&env, 0x91), &SCHEMA_VERSION);
    let upgrade_eta = client.get_pending_upgrade().unwrap().eta_ledger;
    advance_ledger(&env, upgrade_eta - 1);
    assert_eq!(
        client.try_finalize_upgrade(),
        Err(Ok(ContractError::TimelockNotElapsed)),
        "upgrade: one ledger before ETA must fail the timelock gate"
    );
    advance_ledger(&env, upgrade_eta);
    assert_ne!(
        client.try_finalize_upgrade(),
        Err(Ok(ContractError::TimelockNotElapsed)),
        "upgrade: at ETA must not fail the timelock gate"
    );

    // Relay-signer rotation flow.
    let new_relay = Address::generate(&env);
    client.set_relay_rotation_delay(&delay);
    client.propose_relay_rotation(&new_relay);
    let rotation_eta = client.get_pending_relay_rotation().unwrap().eta_ledger;
    advance_ledger(&env, rotation_eta - 1);
    assert_eq!(
        client.try_finalize_relay_rotation(),
        Err(Ok(ContractError::TimelockNotElapsed)),
        "rotation: one ledger before ETA must fail the timelock gate"
    );
    advance_ledger(&env, rotation_eta);
    assert_ne!(
        client.try_finalize_relay_rotation(),
        Err(Ok(ContractError::TimelockNotElapsed)),
        "rotation: at ETA must not fail the timelock gate"
    );
}

#[test]
fn test_no_pending_change_consistent_across_features() {
    // With nothing pending, both finalize and cancel paths must surface
    // NoPendingChange identically for upgrade and relay-signer rotation.
    let (env, client, _) = setup();

    // Upgrade flow: nothing proposed.
    assert!(client.get_pending_upgrade().is_none());
    assert_eq!(
        client.try_finalize_upgrade(),
        Err(Ok(ContractError::NoPendingChange))
    );
    assert_eq!(
        client.try_cancel_upgrade(),
        Err(Ok(ContractError::NoPendingChange))
    );

    // Relay-signer rotation flow: nothing proposed.
    assert!(client.get_pending_relay_rotation().is_none());
    assert_eq!(
        client.try_finalize_relay_rotation(),
        Err(Ok(ContractError::NoPendingChange))
    );
    assert_eq!(
        client.try_cancel_relay_rotation(),
        Err(Ok(ContractError::NoPendingChange))
    );

    // After a cancel, both flows return to the same NoPendingChange state.
    client.set_upgrade_delay(&10);
    client.propose_upgrade(&hash(&env, 0x92), &SCHEMA_VERSION);
    client.cancel_upgrade();
    assert_eq!(
        client.try_finalize_upgrade(),
        Err(Ok(ContractError::NoPendingChange))
    );

    let new_relay = Address::generate(&env);
    client.set_relay_rotation_delay(&10);
    client.propose_relay_rotation(&new_relay);
    client.cancel_relay_rotation();
    assert_eq!(
        client.try_finalize_relay_rotation(),
        Err(Ok(ContractError::NoPendingChange))
    );
}

// ─── #320 UpgradeNotReversible / InvalidSchemaCompatRange guards ─────────────

#[test]
fn test_upgrade_not_reversible_when_schema_downgrade() {
    // #102: a proposed upgrade whose expected schema version is lower than the
    // currently stored schema version must be rejected as not reversible.
    let (env, client, _) = setup();
    client.set_upgrade_delay(&10);
    let lower = SCHEMA_VERSION.saturating_sub(1);
    assert_eq!(
        client.try_propose_upgrade(&hash(&env, 0xa1), &lower),
        Err(Ok(ContractError::UpgradeNotReversible)),
        "schema downgrade must be rejected as not reversible"
    );
    assert!(client.get_pending_upgrade().is_none());
}

#[test]
fn test_upgrade_not_reversible_at_equal_schema_boundary() {
    // Equal schema version is the boundary that must be accepted (not rejected).
    let (env, client, _) = setup();
    client.set_upgrade_delay(&10);
    assert_ne!(
        client.try_propose_upgrade(&hash(&env, 0xa2), &SCHEMA_VERSION),
        Err(Ok(ContractError::UpgradeNotReversible)),
        "equal schema version must not be rejected as not reversible"
    );
    assert!(client.get_pending_upgrade().is_some());
}

#[test]
fn test_upgrade_not_reversible_at_one_below_boundary() {
    // Exactly one below the current schema version is the tightest downgrade
    // boundary and must still be rejected.
    let (env, client, _) = setup();
    client.set_upgrade_delay(&10);
    let one_below = SCHEMA_VERSION - 1;
    assert_eq!(
        client.try_propose_upgrade(&hash(&env, 0xa3), &one_below),
        Err(Ok(ContractError::UpgradeNotReversible))
    );
}

#[test]
fn test_invalid_schema_compat_range_min_greater_than_max() {
    // #55/#104: min > max is the canonical invalid range and must be rejected.
    let (_env, client, _) = setup();
    assert_eq!(
        client.try_set_schema_compat_range(&2, &1),
        Err(Ok(ContractError::InvalidSchemaCompatRange)),
        "min > max must be rejected"
    );
}

#[test]
fn test_invalid_schema_compat_range_min_equals_max_is_valid() {
    // min == max is the tightest valid range and must be accepted.
    let (_env, client, _) = setup();
    assert_ne!(
        client.try_set_schema_compat_range(&1, &1),
        Err(Ok(ContractError::InvalidSchemaCompatRange)),
        "min == max must be accepted"
    );
}

#[test]
fn test_invalid_schema_compat_range_zero_min() {
    // A zero lower bound is outside the ADR's valid schema range and must be
    // rejected as an invalid compat range.
    let (_env, client, _) = setup();
    assert_eq!(
        client.try_set_schema_compat_range(&0, &1),
        Err(Ok(ContractError::InvalidSchemaCompatRange)),
        "zero min must be rejected"
    );
}

#[test]
fn test_bucket9_error_variant_budget() {
    // #15 budget: the full error-variant set must stay within 50 variants.
    // This guards against bucket-9 additions silently blowing the budget.
    let variant_count = ContractError::UpgradeNotReversible as u32;
    assert!(
        variant_count < 50,
        "bucket-9 error variants must fit within the <=50 budget"
    );
}

// ─── #82 upgrade_and_migrate ──────

/* … truncated 3289 chars — edit only what you need near the top … */
