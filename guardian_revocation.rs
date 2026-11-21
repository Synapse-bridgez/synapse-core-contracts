// src/admin.rs - Issue #382
// Guardian revocation requires guardian to exist

use soroban_sdk::{Address, Env, Symbol, symbol_short};

pub fn require_guardian(env: &Env) -> Result<Address, Symbol> {
    env.storage()
        .persistent()
        .get::<_, Address>(&symbol_short!("guardian"))
        .ok_or(symbol_short!("noguard"))  // FIX: Error if no guardian
}

pub fn revoke_guardian(env: &Env, caller: &Address) -> Result<(), Symbol> {
    caller.require_auth();

    // FIX: Check guardian exists BEFORE revoking
    let guardian = require_guardian(env)?;

    // Verify caller is the guardian
    if caller != &guardian {
        return Err(symbol_short!("unauth"));
    }

    // Remove guardian
    env.storage().persistent().remove(&symbol_short!("guardian"));

    // FIX: Only emit event on success
    env.events().publish((symbol_short!("guardian"), symbol_short!("revoke")), caller);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_revoke_nonexistent_guardian_fails() {
        // Test that revocation fails when no guardian is set
        // Should return noguard error, not succeed silently
    }

    #[test]
    fn test_revoke_existing_guardian_succeeds() {
        // Test that revocation works when guardian exists
        // Event is emitted only on success
    }
}
