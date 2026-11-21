// src/admin.rs & src/validation.rs & src/storage.rs - Issues #372, #371, #370, #369
// Validation and storage layer fixes

#![no_std]

use soroban_sdk::{Address, Env, Symbol, symbol_short};

// ============================================================================
// Issue #372: Guardian revocation validation
// ============================================================================

pub fn require_guardian(env: &Env) -> Result<Address, Symbol> {
    env.storage()
        .persistent()
        .get::<_, Address>(&symbol_short!("guardian"))
        .ok_or(symbol_short!("noguard"))  // ERROR: No guardian set
}

pub fn revoke_guardian(env: &Env, caller: &Address) -> Result<(), Symbol> {
    caller.require_auth();

    // FIX: Check guardian exists before revoking
    let guardian = require_guardian(env)?;

    // Verify caller is the guardian
    if caller != &guardian {
        return Err(symbol_short!("unauth"));
    }

    // Remove guardian
    env.storage().persistent().remove(&symbol_short!("guardian"));

    // Emit revoke event
    env.events().publish((symbol_short!("revoke"),), caller);

    Ok(())
}

// ============================================================================
// Issue #371: Pending admin transfer self-transfer rejection
// ============================================================================

pub fn require_admin(env: &Env) -> Result<Address, Symbol> {
    env.storage()
        .persistent()
        .get::<_, Address>(&symbol_short!("admin"))
        .ok_or(symbol_short!("noadmin"))
}

pub fn require_pending_admin(env: &Env, proposed: &Address) -> Result<(), Symbol> {
    // FIX: Reject if proposed admin is the current admin
    let current_admin = require_admin(env)?;

    if proposed == &current_admin {
        return Err(symbol_short!("samead"));  // ERROR: Cannot transfer to self
    }

    // Store pending admin
    env.storage()
        .persistent()
        .set(&symbol_short!("pending_admin"), proposed);

    Ok(())
}

pub fn accept_admin_transfer(env: &Env, caller: &Address) -> Result<(), Symbol> {
    caller.require_auth();

    // Verify caller is the pending admin
    let pending: Address = env.storage()
        .persistent()
        .get(&symbol_short!("pending_admin"))
        .ok_or(symbol_short!("nopending"))?;

    if caller != &pending {
        return Err(symbol_short!("unauth"));
    }

    // Complete transfer
    env.storage().persistent().set(&symbol_short!("admin"), caller);
    env.storage().persistent().remove(&symbol_short!("pending_admin"));

    Ok(())
}

// ============================================================================
// Issue #370: Treasury withdrawal balance validation
// ============================================================================

pub fn treasury_balance(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get::<_, i128>(&symbol_short!("treas_bal"))
        .unwrap_or(0)
}

pub fn set_treasury_balance(env: &Env, amount: i128) -> Result<(), Symbol> {
    // FIX: Reject negative balances (underflow)
    if amount < 0 {
        return Err(symbol_short!("underflow"));
    }

    env.storage()
        .persistent()
        .set(&symbol_short!("treas_bal"), &amount);

    Ok(())
}

pub fn withdraw_treasury(env: &Env, amount: i128) -> Result<(), Symbol> {
    // FIX: Validate balance is sufficient BEFORE attempting withdrawal
    let current = treasury_balance(env);

    if amount > current {
        return Err(symbol_short!("insuf"));  // ERROR: Insufficient balance
    }

    // Safe subtraction
    let remaining = current - amount;

    // Update balance
    set_treasury_balance(env, remaining)?;

    Ok(())
}

// ============================================================================
// Issue #369: Reject zero-amount transfers in validation
// ============================================================================

pub fn validate_amount(env: &Env, amount: i128) -> Result<(), Symbol> {
    // FIX: Reject zero amounts at validation boundary
    if amount == 0 {
        return Err(symbol_short!("badamt"));  // ERROR: Amount must be > 0
    }

    // Check upper bound (ceiling)
    let ceiling: i128 = env.storage()
        .persistent()
        .get(&symbol_short!("ceiling"))
        .unwrap_or(i128::MAX);

    if amount > ceiling {
        return Err(symbol_short!("exceed"));  // ERROR: Exceeds ceiling
    }

    Ok(())
}

pub fn transfer_validated(env: &Env, _from: &Address, _to: &Address, amount: i128) -> Result<(), Symbol> {
    // Single validation point - all amount checks here
    validate_amount(env, amount)?;

    // Transfer logic follows validation...
    Ok(())
}

// ============================================================================
// Tests for all fixes
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::*, Env as TestEnv, map};

    #[test]
    fn test_revoke_nonexistent_guardian_fails() {
        let env = TestEnv::default();
        let caller = Address::random(&env);

        let result = revoke_guardian(&env, &caller);
        assert_eq!(result, Err(symbol_short!("noguard")));
    }

    #[test]
    fn test_self_admin_transfer_rejected() {
        let env = TestEnv::default();
        let admin = Address::random(&env);

        // Set current admin
        env.storage().persistent().set(&symbol_short!("admin"), &admin);

        // Try to transfer to self
        let result = require_pending_admin(&env, &admin);
        assert_eq!(result, Err(symbol_short!("samead")));
    }

    #[test]
    fn test_treasury_underflow_rejected() {
        let env = TestEnv::default();

        // Set balance to 50
        env.storage().persistent().set(&symbol_short!("treas_bal"), &50i128);

        // Try to withdraw 100 (underflow)
        let result = withdraw_treasury(&env, 100);
        assert_eq!(result, Err(symbol_short!("insuf")));
    }

    #[test]
    fn test_zero_amount_rejected() {
        let env = TestEnv::default();

        let result = validate_amount(&env, 0);
        assert_eq!(result, Err(symbol_short!("badamt")));
    }

    #[test]
    fn test_positive_amount_accepted() {
        let env = TestEnv::default();

        let result = validate_amount(&env, 100);
        assert!(result.is_ok());
    }

    #[test]
    fn test_negative_balance_rejected() {
        let env = TestEnv::default();

        let result = set_treasury_balance(&env, -10);
        assert_eq!(result, Err(symbol_short!("underflow")));
    }
}
