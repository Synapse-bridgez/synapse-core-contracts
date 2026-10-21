use soroban_sdk::{contracttype, BytesN};

/// Schema version of the contract's persistent storage layout.
///
/// Bumped whenever the on-chain storage shape changes so that
/// `post_upgrade_self_check` can detect a mismatch between the running
/// WASM and the data it is expected to read.
pub const SCHEMA_VERSION: u32 = 1;

/// A minimal capture of critical pre-upgrade state.
///
/// Taken immediately before `upgrade()` / `upgrade_and_migrate()` swap the
/// WASM so that `post_upgrade_self_check` (issue #26/#97) can verify the new
/// code against the state it inherited, and `rollback_upgrade` (issue #102)
/// can reverse a bad upgrade.
///
/// The snapshot is intentionally small and cheap to construct because it is
/// captured on every upgrade attempt, not just ones that end up needing a
/// rollback. It must be persisted in storage that survives the WASM swap
/// (persistent or instance storage), never in anything tied to the old WASM.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeSnapshot {
    /// Hash of the WASM that was live immediately before the upgrade.
    pub wasm_hash: BytesN<32>,
    /// Storage schema version the pre-upgrade WASM was operating under.
    pub schema_version: u32,
    /// Ledger sequence at which the snapshot was captured.
    pub captured_at: u32,
}

impl UpgradeSnapshot {
    /// Construct a snapshot from the pre-upgrade WASM hash and the ledger
    /// sequence at capture time, using the current [`SCHEMA_VERSION`].
    pub fn capture(wasm_hash: BytesN<32>, captured_at: u32) -> Self {
        Self {
            wasm_hash,
            schema_version: SCHEMA_VERSION,
            captured_at,
        }
    }

    /// Returns `true` when the snapshot's schema version matches the running
    /// contract's [`SCHEMA_VERSION`].
    pub fn schema_matches(&self) -> bool {
        self.schema_version == SCHEMA_VERSION
    }
}
