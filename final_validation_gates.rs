// Issues #377, #380, #379, #378 - Validation Gates

// ============================================================================
// Issue #377: Treasury balance read failure handling
// ============================================================================

pub enum TreasuryError {
    NotConfigured,
    InsufficientBalance,
}

pub fn get_treasury_balance(env: &soroban_sdk::Env) -> Result<i128, TreasuryError> {
    // FIX: Distinguish missing config from zero balance
    env.storage()
        .persistent()
        .get::<_, i128>(&symbol_short!("treas_bal"))
        .ok_or(TreasuryError::NotConfigured)
}

pub fn withdraw_from_treasury(env: &soroban_sdk::Env, amount: i128) -> Result<(), TreasuryError> {
    // FIX: Check config exists first
    let _balance = get_treasury_balance(env)?;  // Returns NotConfigured if missing

    if _balance < amount {
        return Err(TreasuryError::InsufficientBalance);
    }

    Ok(())
}

use soroban_sdk::symbol_short;

// ============================================================================
// Issue #380: WASM size gate checks artifact exists
// ============================================================================

pub fn check_wasm_size(wasm_path: &str, limit_path: &str) -> Result<(), String> {
    use std::fs;
    use std::path::Path;

    // FIX: Fail if WASM artifact is missing
    if !Path::new(wasm_path).exists() {
        return Err(format!("WASM artifact not found: {}", wasm_path));
    }

    // Check size against limit
    let metadata = fs::metadata(wasm_path)
        .map_err(|e| format!("Failed to read WASM: {}", e))?;

    let size = metadata.len();

    // Parse limit
    let limit_str = fs::read_to_string(limit_path)
        .map_err(|e| format!("Failed to read limit: {}", e))?;

    let limit: u64 = limit_str.trim().parse()
        .map_err(|e| format!("Invalid limit: {}", e))?;

    if size > limit {
        return Err(format!("WASM size {} exceeds limit {}", size, limit));
    }

    Ok(())
}

// ============================================================================
// Issue #379: Schema migration manifest required
// ============================================================================

pub fn check_schema_migrations(manifest_path: &str) -> Result<(), String> {
    use std::fs;
    use std::path::Path;

    // FIX: Fail if manifest is missing
    if !Path::new(manifest_path).exists() {
        return Err(format!("Migration manifest not found: {}", manifest_path));
    }

    // Parse and validate manifest
    let content = fs::read_to_string(manifest_path)
        .map_err(|e| format!("Failed to read manifest: {}", e))?;

    // Check for migrations (simplified)
    if content.trim().is_empty() {
        return Err("Migration manifest is empty".to_string());
    }

    Ok(())
}

// ============================================================================
// Issue #378: Resource baseline parser rejects malformed input
// ============================================================================

#[derive(Clone)]
pub struct Cost {
    pub name: String,
    pub value: u64,
}

pub struct Baseline {
    pub costs: Vec<Cost>,
}

pub fn parse_baseline(content: &str) -> Result<Baseline, String> {
    let mut costs = Vec::new();

    for (line_no, line) in content.lines().enumerate() {
        let line = line.trim();

        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Parse format: "name = value"
        let parts: Vec<&str> = line.split('=').collect();
        if parts.len() != 2 {
            // FIX: Return error instead of silently skipping
            return Err(format!("Line {}: Invalid format: {}", line_no + 1, line));
        }

        let name = parts[0].trim().to_string();
        let value_str = parts[1].trim();

        let value: u64 = value_str.parse()
            .map_err(|_| format!("Line {}: Invalid value: {}", line_no + 1, value_str))?;

        costs.push(Cost { name, value });
    }

    Ok(Baseline { costs })
}

pub fn check_resource_budget(baseline: &Baseline, actual_costs: &[Cost]) -> Result<(), String> {
    for cost in actual_costs {
        if let Some(baseline_cost) = baseline.costs.iter().find(|c| c.name == cost.name) {
            if cost.value > baseline_cost.value {
                return Err(format!("Budget exceeded for {}: {} > {}",
                    cost.name, cost.value, baseline_cost.value));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_malformed_baseline_rejected() {
        let malformed = "name1 = 100\ninvalid line\nname2 = 200";
        assert!(parse_baseline(malformed).is_err());
    }

    #[test]
    fn test_valid_baseline_accepted() {
        let valid = "name1 = 100\nname2 = 200";
        assert!(parse_baseline(valid).is_ok());
    }
}
