//! # Migration registry
//!
//! Versioned, auditable on-chain migration routines invoked by
//! [`crate::SynapseCoreContract::upgrade_and_migrate`].
//!
//! ## Design (ADR-0005)
//!
//! * Each `migration_id` maps to exactly one transformation shipped in this
//!   WASM. Unknown IDs fail closed with [`ContractError::UnknownMigration`].
//! * A single call may touch at most [`MAX_MIGRATION_STORAGE_TOUCHES`]
//!   storage entries. Exceeding the cap fails with
//!   [`ContractError::MigrationFailed`] before any further work —
//!   so simulation/dry-run surfaces the failure rather than partial
//!   mainnet execution.
//! * Migrations larger than one call require a resumable multi-call state
//!   machine; that is **out of scope for v1** and must be designed per
//!   ADR-0005 before any production migration that would exceed the bound.
//! * On any `Err`, Soroban aborts the invocation and rolls storage back, so
//!   a failed migration leaves the ledger byte-for-byte identical to the
//!   pre-call state (and the WASM swap never runs).

use soroban_sdk::{contracttype, BytesN, Env};

use crate::storage::StorageClient;
use crate::types::ContractError;

/// Hard cap on persistent/instance keys a single migration invocation may
/// read or write. Chosen to stay well inside typical Soroban resource
/// limits while still allowing small additive schema transforms.
pub const MAX_MIGRATION_STORAGE_TOUCHES: u32 = 64;

/// Sentinel `migration_id` for a no-op migration (useful for exercising the
/// atomic upgrade+migrate path without transforming storage).
pub const MIGRATION_NOOP: u32 = 0;

/// Test-only / scaffolding id: writes one marker key then succeeds.
/// Real schema migrations claim subsequent ids in this match arms list.
pub const MIGRATION_WRITE_MARKER: u32 = 1;

/// Test-only id that writes a marker then fails — used to prove atomic
/// revert leaves storage unchanged.
pub const MIGRATION_FAIL_AFTER_WRITE: u32 = 2;

/// Minimal capture of critical pre-upgrade state, taken immediately before
/// `upgrade()` / `upgrade_and_migrate()` swap the WASM.
///
/// Consumed by `post_upgrade_self_check` (issues #26/#97) to verify the new
/// WASM against the state it replaced, and by `rollback_upgrade` (issue #102)
/// to reverse a bad upgrade. Kept deliberately small and cheap to construct
/// since it is captured on every upgrade attempt, not just ones that end up
/// needing rollback.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeSnapshot {
    /// Hash of the WASM that was live immediately before the upgrade.
    pub wasm_hash: BytesN<32>,
    /// Schema version recorded before the upgrade.
    pub schema_version: u32,
    /// Ledger sequence at which the snapshot was captured.
    pub captured_at_ledger: u32,
}

impl UpgradeSnapshot {
    /// Capture the current pre-upgrade state into a snapshot.
    ///
    /// Reads the live WASM hash and schema version from storage and stamps
    /// the current ledger sequence. Cheap: a constant number of reads.
    pub fn capture(env: &Env) -> Self {
        Self {
            wasm_hash: StorageClient::get_wasm_hash(env),
            schema_version: StorageClient::get_schema_version(env),
            captured_at_ledger: env.ledger().sequence(),
        }
    }

    /// Persist this snapshot to instance storage so it survives the WASM
    /// swap it is meant to let the contract recover from.
    pub fn store(&self, env: &Env) {
        StorageClient::set_upgrade_snapshot(env, self);
    }

    /// Load the most recently captured snapshot, if any.
    pub fn load(env: &Env) -> Option<Self> {
        StorageClient::get_upgrade_snapshot(env)
    }

    /// Capture the current pre-upgrade state and persist it in one step.
    /// Call this immediately before swapping the WASM.
    pub fn capture_and_store(env: &Env) -> Self {
        let snapshot = Self::capture(env);
        snapshot.store(env);
        snapshot
    }
}

pub struct MigrationRegistry;

impl MigrationRegistry {
    /// Dispatch `migration_id` to its routine. Returns the number of storage
    /// touches performed (for audit / events).
    pub fn run(env: &Env, migration_id: u32) -> Result<u32, ContractError> {
        match migration_id {
            MIGRATION_NOOP => Ok(0),
            MIGRATION_WRITE_MARKER => {
                Self::assert_within_bound(1)?;
                StorageClient::set_migration_marker(env, migration_id);
                Ok(1)
            }
            MIGRATION_FAIL_AFTER_WRITE => {
                Self::assert_within_bound(1)?;
                StorageClient::set_migration_marker(env, migration_id);
                // Deliberate failure after a write — caller must observe
                // full rollback of this marker when the invoke returns Err.
                Err(ContractError::MigrationFailed)
            }
            _ => Err(ContractError::UnknownMigration),
        }
    }

    fn assert_within_bound(planned_touches: u32) -> Result<(), ContractError> {
        if planned_touches > MAX_MIGRATION_STORAGE_TOUCHES {
            return Err(ContractError::MigrationFailed);
        }
        Ok(())
    }
}
