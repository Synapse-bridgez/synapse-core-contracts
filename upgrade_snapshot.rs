// src/migration.rs - Issue #374
// Upgrade snapshot capture respects schema version guard

use soroban_sdk::{Env, Symbol, symbol_short};

#[derive(Debug, Clone)]
pub struct SchemaDiff {
    pub changes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct UpgradeSnapshot {
    pub from_version: u32,
    pub to_version: u32,
    pub schema_diff: SchemaDiff,
}

pub fn compare_schema_versions(old_version: u32, new_version: u32) -> Result<SchemaDiff, Symbol> {
    // Check compatibility per ADR-0003
    if new_version < old_version {
        return Err(symbol_short!("downgrade"));
    }

    // Additional compatibility checks would go here
    if old_version == 1 && new_version == 1 {
        // No changes
        Ok(SchemaDiff { changes: vec![] })
    } else if new_version > old_version {
        // Forward version - check if compatible
        Ok(SchemaDiff {
            changes: vec![format!("Schema upgrade from {} to {}", old_version, new_version)],
        })
    } else {
        Err(symbol_short!("badversion"))
    }
}

pub fn capture(
    env: &Env,
    old_version: u32,
    new_version: u32,
) -> Result<UpgradeSnapshot, Symbol> {
    // FIX: Call compare_schema_versions BEFORE capturing snapshot
    // This ensures the schema guard is respected per ADR-0003
    let schema_diff = compare_schema_versions(old_version, new_version)?;

    // Only after schema check passes, capture the snapshot
    Ok(UpgradeSnapshot {
        from_version: old_version,
        to_version: new_version,
        schema_diff,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_checks_schema_guard() {
        let env = soroban_sdk::testutils::Env::default();

        // Invalid downgrade should fail before snapshot
        let result = capture(&env, 2, 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_capture_succeeds_with_compatible_upgrade() {
        let env = soroban_sdk::testutils::Env::default();

        // Valid upgrade should succeed
        let result = capture(&env, 1, 2);
        assert!(result.is_ok());

        let snapshot = result.unwrap();
        assert_eq!(snapshot.from_version, 1);
        assert_eq!(snapshot.to_version, 2);
    }

    #[test]
    fn test_snapshot_not_created_on_schema_error() {
        let env = soroban_sdk::testutils::Env::default();

        // This should fail and NOT create a snapshot
        let result = capture(&env, 2, 1);
        assert!(result.is_err());
        // If capture() didn't check the guard first, the snapshot would be created
    }
}
