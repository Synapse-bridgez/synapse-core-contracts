//! Migration-metadata manifest and schema-version diffing.
//!
//! This module provides the on-chain-adjacent metadata used by the off-chain
//! `compare_schema_versions()` tooling. It records, per schema version, the
//! additive storage/semantic changes introduced by that version so operators
//! planning a multi-version upgrade can mechanically assemble the cumulative
//! diff instead of reading prose in `CHANGELOG.md`/`DECISIONS.md`.
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
//!
//! ## Reversibility (issue #314)
//!
//! [`MigrationRegistry::is_reversible`] classifies a migration as reversible
//! only when it is a purely additive / no-op transform. Structural schema
//! changes (anything that rewrites or removes existing keys) cannot be
//! reverted cleanly by restoring the previous WASM hash, so
//! [`crate::SynapseCoreContract::rollback_upgrade`] refuses them with
//! [`ContractError::UpgradeNotReversible`].
//!
//! The tooling here is strictly read-only and informational: it never performs
//! or automates an upgrade.

use soroban_sdk::{contracttype, Env, String, Vec};

/// A single documented change introduced by a schema version.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaChange {
    /// The schema version that introduced this change.
    pub version: u32,
    /// Short machine-readable kind, e.g. `"storage_key"`, `"encoding"`.
    pub kind: String,
    /// Human-readable description of the change.
    pub description: String,
}

/// The migration-metadata manifest: an ordered list of documented changes.
///
/// Entries are expected to be ordered by ascending `version`. The manifest is
/// the single data source for the off-chain diff tool.
pub type MigrationManifest = Vec<SchemaChange>;

/// Result of comparing two schema versions.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaDiff {
    /// The lower (source) schema version of the comparison.
    pub from_version: u32,
    /// The higher (target) schema version of the comparison.
    pub to_version: u32,
    /// Changes with `from_version < version <= to_version`, in order.
    pub changes: Vec<SchemaChange>,
    /// True when no additive changes were recorded in the range.
    pub empty: bool,
}

/// Compare two schema versions against the migration manifest.
///
/// Returns the ordered list of changes introduced strictly after
/// `from_version` and up to and including `to_version`. When the range contains
/// no manifest entries the returned [`SchemaDiff`] has `empty == true` and an
/// empty `changes` list — callers should render a clear "no additive changes
/// recorded" message rather than treating this as an error.
///
/// This is a read-only, informational helper for operator planning; it does not
/// perform or automate any upgrade.
pub fn compare_schema_versions(
    env: &Env,
    manifest: &MigrationManifest,
    from_version: u32,
    to_version: u32,
) -> SchemaDiff {
    let (lo, hi) = if from_version <= to_version {
        (from_version, to_version)
    } else {
        (to_version, from_version)
    };

    let mut changes: Vec<SchemaChange> = Vec::new(env);
    for change in manifest.iter() {
        if change.version > lo && change.version <= hi {
            changes.push_back(change);
        }
    }
        }
    }

    let empty = changes.is_empty();
    SchemaDiff {
        from_version: lo,
        to_version: hi,
        changes,
        empty,
    }
}
