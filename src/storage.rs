use soroban_sdk::{contracttype, BytesN, Env};

/// Persistent key under which the pre-upgrade [`UpgradeSnapshot`] is stored.
///
/// Kept in persistent storage so the snapshot survives the WASM swap it is
/// meant to let us recover from (issue #308).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageKey {
    /// Pre-upgrade state captured immediately before a WASM swap.
    UpgradeSnapshot,
}

/// Minimal capture of critical pre-upgrade state.
///
/// Taken immediately before `upgrade()` / `upgrade_and_migrate()` swap the
/// WASM so that `post_upgrade_self_check` (issue #26/#97) can verify the new
/// code against the state it replaced, and `rollback_upgrade` (issue #102)
/// can reverse a bad upgrade. Cheap to construct: it is captured on every
/// upgrade attempt, not just ones that end up needing rollback.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeSnapshot {
    /// WASM hash that was live before the upgrade.
    pub wasm_hash: BytesN<32>,
    /// Contract schema version before the upgrade.
    pub schema_version: u32,
    /// Ledger sequence at which the snapshot was captured.
    pub captured_at: u32,
}

impl UpgradeSnapshot {
    /// Capture the current pre-upgrade state.
    ///
    /// `wasm_hash` is the hash of the currently installed WASM and
    /// `schema_version` the current schema version; both are supplied by the
    /// caller since they are read from the live contract state.
    pub fn capture(env: &Env, wasm_hash: BytesN<32>, schema_version: u32) -> Self {
        Self {
            wasm_hash,
            schema_version,
            captured_at: env.ledger().sequence(),
        }
    }

    /// Persist this snapshot so it survives the WASM swap.
    pub fn store(&self, env: &Env) {
        env.storage()
            .persistent()
            .set(&StorageKey::UpgradeSnapshot, self);
    }

    /// Load the stored snapshot, if any.
    pub fn load(env: &Env) -> Option<Self> {
        env.storage()
            .persistent()
            .get(&StorageKey::UpgradeSnapshot)
    }

    /// Remove the stored snapshot (e.g. after a successful self-check).
    pub fn clear(env: &Env) {
        env.storage()
            .persistent()
            .remove(&StorageKey::UpgradeSnapshot);
    }
}
