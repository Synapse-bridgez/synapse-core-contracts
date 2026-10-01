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

use soroban_sdk::Env;

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
