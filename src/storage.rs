//! Storage helpers for the contract.
//!
//! Centralises the storage keys and accessors used across the contract so
//! that the auth, upgrade, and migration flows all read/write the same
//! locations. See `docs/adr/0004-timelocked-upgrade.md` for the timelocked
//! upgrade state machine.

use soroban_sdk::{contracttype, Address, BytesN, Env};

/// Storage keys used by the contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Contract administrator address.
    Admin,
    /// Pending admin transfer proposal (issue #5).
    PendingAdmin,
    /// Pending relay-signer rotation proposal (issue #87).
    PendingRelaySigner,
    /// Pending timelocked upgrade proposal (issue #312).
    PendingUpgrade,
    /// Ledger timestamp at which the pending upgrade becomes finalizable.
    UpgradeReadyAt,
    /// Current schema version of the stored contract state.
    SchemaVersion,
}

/// A pending timelocked upgrade proposal.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingUpgrade {
    /// The new WASM hash to install on finalize.
    pub new_wasm_hash: BytesN<32>,
    /// The schema version the new WASM is expected to produce.
    pub expected_schema_version: u32,
}

/// Returns the configured upgrade delay in seconds (issue #84/#105).
pub fn upgrade_delay(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::UpgradeDelay)
        .unwrap_or(0)
}

/// Stores a pending upgrade proposal and starts the timelock.
///
/// The proposal becomes finalizable once `upgrade_delay()` seconds have
/// elapsed from the current ledger timestamp.
pub fn set_pending_upgrade(env: &Env, proposal: &PendingUpgrade) {
    let ready_at = env.ledger().timestamp().saturating_add(upgrade_delay(env));
    env.storage()
        .instance()
        .set(&DataKey::PendingUpgrade, proposal);
    env.storage()
        .instance()
        .set(&DataKey::UpgradeReadyAt, &ready_at);
}

/// Returns the pending upgrade proposal, if any.
pub fn get_pending_upgrade(env: &Env) -> Option<PendingUpgrade> {
    env.storage().instance().get(&DataKey::PendingUpgrade)
}

/// Returns the ledger timestamp at which the pending upgrade becomes
/// finalizable, if a proposal is pending.
pub fn get_upgrade_ready_at(env: &Env) -> Option<u64> {
    env.storage().instance().get(&DataKey::UpgradeReadyAt)
}

/// Clears any pending upgrade proposal and its timelock.
pub fn clear_pending_upgrade(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingUpgrade);
    env.storage().instance().remove(&DataKey::UpgradeReadyAt);
}

/// Returns the current schema version of the stored contract state.
pub fn get_schema_version(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::SchemaVersion)
        .unwrap_or(0)
}

/// Persists the schema version of the stored contract state.
pub fn set_schema_version(env: &Env, version: u32) {
    env.storage()
        .instance()
        .set(&DataKey::SchemaVersion, &version);
}

/// Returns the current administrator address.
pub fn get_admin(env: &Env) -> Address {
    env.storage().instance().get(&DataKey::Admin).unwrap()
}
