// src/migration.rs - Issue #373
// Schema diff treats removed field as incompatible

use soroban_sdk::Symbol;

#[derive(Debug, Clone, PartialEq)]
pub enum SchemaChange {
    FieldAdded { name: String },
    FieldRemoved { name: String },
    FieldTypeChanged { name: String },
}

pub fn is_change_compatible(change: &SchemaChange) -> bool {
    match change {
        SchemaChange::FieldAdded { .. } => true,  // Forward compatible
        SchemaChange::FieldRemoved { .. } => false,  // FIX: INCOMPATIBLE
        SchemaChange::FieldTypeChanged { .. } => false,  // Incompatible
    }
}

#[derive(Debug, Clone)]
pub struct SchemaField {
    pub name: String,
    pub field_type: String,
}

#[derive(Debug, Clone)]
pub struct Schema {
    pub version: u32,
    pub fields: Vec<SchemaField>,
}

pub fn compare_schema_versions(old: &Schema, new: &Schema) -> Result<Vec<SchemaChange>, String> {
    let mut changes = Vec::new();

    // Detect removed fields (must be incompatible)
    for old_field in &old.fields {
        if !new.fields.iter().any(|f| f.name == old_field.name) {
            let removal = SchemaChange::FieldRemoved {
                name: old_field.name.clone(),
            };

            // FIX: Check compatibility immediately
            if !is_change_compatible(&removal) {
                return Err(format!("Incompatible change: field '{}' removed", old_field.name));
            }

            changes.push(removal);
        }
    }

    // Detect added fields (forward compatible)
    for new_field in &new.fields {
        if !old.fields.iter().any(|f| f.name == new_field.name) {
            changes.push(SchemaChange::FieldAdded {
                name: new_field.name.clone(),
            });
        }
    }

    // Detect type changes (incompatible)
    for old_field in &old.fields {
        if let Some(new_field) = new.fields.iter().find(|f| f.name == old_field.name) {
            if old_field.field_type != new_field.field_type {
                return Err(format!(
                    "Incompatible change: field '{}' type changed from {} to {}",
                    old_field.name, old_field.field_type, new_field.field_type
                ));
            }
        }
    }

    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_removed_field_is_incompatible() {
        let old = Schema {
            version: 1,
            fields: vec![SchemaField {
                name: "field1".into(),
                field_type: "u32".into(),
            }],
        };

        let new = Schema {
            version: 2,
            fields: vec![],  // Field removed
        };

        let result = compare_schema_versions(&old, &new);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Incompatible"));
    }

    #[test]
    fn test_added_field_is_compatible() {
        let old = Schema {
            version: 1,
            fields: vec![],
        };

        let new = Schema {
            version: 2,
            fields: vec![SchemaField {
                name: "new_field".into(),
                field_type: "u32".into(),
            }],
        };

        let result = compare_schema_versions(&old, &new);
        assert!(result.is_ok());
    }
}
