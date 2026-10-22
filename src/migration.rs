//! Migration-metadata manifest and schema-version diffing.
//!
//! This module provides the on-chain-adjacent metadata used by the off-chain
//! `compare_schema_versions()` tooling. It records, per schema version, the
//! additive storage/semantic changes introduced by that version so operators
//! planning a multi-version upgrade can mechanically assemble the cumulative
//! diff instead of reading prose in `CHANGELOG.md`/`DECISIONS.md`.
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

    let empty = changes.is_empty();
    SchemaDiff {
        from_version: lo,
        to_version: hi,
        changes,
        empty,
    }
}
