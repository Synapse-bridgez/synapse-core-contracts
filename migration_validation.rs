// src/migration.rs & src/storage.rs - Issues #375, #376, #374, #373

#![no_std]

use soroban_sdk::{Symbol, symbol_short};
use std::collections::HashSet;

// ============================================================================
// Issue #375: Deduplicate tags before counting
// ============================================================================

const MAX_TAGS_PER_TRANSACTION: usize = 50;

pub fn validate_tags(tags: &[Symbol]) -> Result<(), Symbol> {
    // FIX: Deduplicate before counting
    let distinct_tags: HashSet<_> = tags.iter().collect();

    if distinct_tags.len() > MAX_TAGS_PER_TRANSACTION {
        return Err(symbol_short!("toomanytags"));
    }

    Ok(())
}

// ============================================================================
// Issue #373: Schema diff treats removed field as incompatible
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum SchemaChange {
    FieldAdded { name: String },
    FieldRemoved { name: String },  // FIX: This is INCOMPATIBLE
    FieldTypeChanged { name: String },
}

#[derive(Debug, Clone)]
pub struct SchemaDiff {
    pub changes: Vec<SchemaChange>,
}

pub fn classify_compatibility(change: &SchemaChange) -> bool {
    match change {
        SchemaChange::FieldAdded { .. } => true,  // Compatible: forward compatible
        SchemaChange::FieldRemoved { .. } => false,  // FIX: INCOMPATIBLE - readers expect field
        SchemaChange::FieldTypeChanged { .. } => false,  // Incompatible
    }
}

pub fn compare_schema_versions(old: &Schema, new: &Schema) -> Result<SchemaDiff, Symbol> {
    let mut changes = Vec::new();

    // Find removed fields
    for field in &old.fields {
        if !new.fields.iter().any(|f| f.name == field.name) {
            changes.push(SchemaChange::FieldRemoved {
                name: field.name.clone(),
            });
        }
    }

    // Find added fields
    for field in &new.fields {
        if !old.fields.iter().any(|f| f.name == field.name) {
            changes.push(SchemaChange::FieldAdded {
                name: field.name.clone(),
            });
        }
    }

    // Check compatibility
    for change in &changes {
        if !classify_compatibility(change) {
            return Err(symbol_short!("incompatible"));
        }
    }

    Ok(SchemaDiff { changes })
}

// ============================================================================
// Issue #374: Upgrade snapshot capture respects schema guard
// ============================================================================

#[derive(Debug, Clone)]
pub struct Schema {
    pub version: u32,
    pub fields: Vec<SchemaField>,
}

#[derive(Debug, Clone)]
pub struct SchemaField {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct UpgradeSnapshot {
    pub from_version: u32,
    pub to_version: u32,
    pub schema_diff: SchemaDiff,
}

pub fn capture(env: &Env, old_schema: &Schema, new_schema: &Schema) -> Result<UpgradeSnapshot, Symbol> {
    // FIX: Check schema version guard BEFORE capturing snapshot
    let schema_diff = compare_schema_versions(old_schema, new_schema)?;

    Ok(UpgradeSnapshot {
        from_version: old_schema.version,
        to_version: new_schema.version,
        schema_diff,
    })
}

// ============================================================================
// Issue #376: Document error codes that are actually returned
// ============================================================================

pub enum ContractError {
    // These should map to docs/error-codes.md
    TagLimitExceeded = 1,         // MAX_TAGS_PER_TRANSACTION
    SchemaIncompatible = 2,       // compare_schema_versions
    InsufficientBalance = 3,      // treasury operations
    UnauthorizedAdmin = 4,        // permission checks
    InvalidState = 5,             // state machine
    // Remove any error codes from docs that don't appear here
}

// Error documentation mapping
// docs/error-codes.md should contain:
// - TagLimitExceeded (1): Too many distinct tags in transaction
// - SchemaIncompatible (2): Upgrade schema diff is not forward compatible
// - InsufficientBalance (3): Insufficient balance for operation
// - UnauthorizedAdmin (4): Caller is not authorized admin
// - InvalidState (5): Invalid contract state for operation
//
// Remove from docs:
// - Any error code NOT in this enum
// - Any code that is documented but never returned

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duplicate_tags_allowed_if_distinct_count_ok() {
        let tags = vec![
            symbol_short!("tag1"),
            symbol_short!("tag1"),  // Duplicate
            symbol_short!("tag2"),
        ];

        // Should succeed: only 2 distinct tags
        assert!(validate_tags(&tags).is_ok());
    }

    #[test]
    fn test_too_many_distinct_tags_rejected() {
        let mut tags = Vec::new();
        for i in 0..=MAX_TAGS_PER_TRANSACTION {
            tags.push(format!("tag{}", i).into());
        }

        // Should fail: too many distinct tags
        assert!(validate_tags(&tags).is_err());
    }

    #[test]
    fn test_removed_field_incompatible() {
        let old_schema = Schema {
            version: 1,
            fields: vec![SchemaField { name: "field1".into() }],
        };

        let new_schema = Schema {
            version: 2,
            fields: vec![],  // Field removed
        };

        let result = compare_schema_versions(&old_schema, &new_schema);
        assert!(result.is_err());  // Should be incompatible
    }

    #[test]
    fn test_added_field_compatible() {
        let old_schema = Schema {
            version: 1,
            fields: vec![],
        };

        let new_schema = Schema {
            version: 2,
            fields: vec![SchemaField { name: "new_field".into() }],
        };

        let result = compare_schema_versions(&old_schema, &new_schema);
        assert!(result.is_ok());  // Forward compatible
    }
}
