// Issues #244, #243, #242, #241 - Storage and Event Restoration

#![no_std]

use soroban_sdk::{Env, Address, Symbol, symbol_short};

// ============================================================================
// Issue #244: Restore EventTransactionCancelled
// ============================================================================

pub fn emit_transaction_cancelled(env: &Env, tx_id: &Symbol, reason: &Symbol, caller: &Address) {
    env.events().publish(
        (symbol_short!("txn"), symbol_short!("cancel")),
        (tx_id, reason, caller),
    );
}

// ============================================================================
// Issue #243: Restore events.rs and EventEmitter pattern
// ============================================================================

pub trait EventEmitter {
    fn emit_event(&self, topic: Symbol, data: &str);
}

pub struct ContractEventEmitter;

impl EventEmitter for ContractEventEmitter {
    fn emit_event(&self, topic: Symbol, data: &str) {
        // Emitter implementation
    }
}

// Core event emissions
pub fn emit_register_callback(env: &Env, tx_id: &Symbol, timestamp: u64) {
    env.events().publish((symbol_short!("register"),), (tx_id, timestamp));
}

pub fn emit_execute_transaction(env: &Env, tx_id: &Symbol, status: &Symbol) {
    env.events().publish((symbol_short!("execute"),), (tx_id, status));
}

pub fn emit_dispute_raised(env: &Env, tx_id: &Symbol, disputer: &Address) {
    env.events().publish((symbol_short!("dispute"),), (tx_id, disputer));
}

// ============================================================================
// Issue #242: Restore treasury balance/config storage
// ============================================================================

#[derive(Clone)]
pub struct TreasuryConfig {
    pub base_fee_bps: u32,
    pub treasury_epoch_cap: i128,
    pub treasury_epoch_length: u64,
}

pub fn get_treasury_balance(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get::<_, i128>(&symbol_short!("treas_bal"))
        .unwrap_or(0)
}

pub fn set_treasury_balance(env: &Env, balance: i128) {
    env.storage()
        .persistent()
        .set(&symbol_short!("treas_bal"), &balance);
}

pub fn get_treasury_config(env: &Env) -> Option<TreasuryConfig> {
    env.storage()
        .persistent()
        .get(&symbol_short!("treas_cfg"))
}

pub fn set_treasury_config(env: &Env, config: &TreasuryConfig) {
    env.storage()
        .persistent()
        .set(&symbol_short!("treas_cfg"), config);
}

pub fn get_pending_withdrawal(env: &Env) -> Option<i128> {
    env.storage()
        .persistent()
        .get::<_, i128>(&symbol_short!("pend_with"))
}

pub fn set_pending_withdrawal(env: &Env, amount: i128) {
    env.storage()
        .persistent()
        .set(&symbol_short!("pend_with"), &amount);
}

// ============================================================================
// Issue #241: Restore collateral-bond-record storage
// ============================================================================

#[derive(Clone)]
pub struct BondRecord {
    pub signer: Address,
    pub amount: i128,
    pub timestamp: u64,
}

#[derive(Clone)]
pub struct UnbondRequest {
    pub signer: Address,
    pub amount: i128,
    pub requested_at: u64,
    pub available_at: u64,
}

pub fn get_bond_record(env: &Env, signer: &Address) -> Option<BondRecord> {
    let key = (symbol_short!("bond"), signer);
    env.storage().persistent().get(&key)
}

pub fn bond_collateral(env: &Env, signer: &Address, amount: i128) {
    // FIX: Top-up semantics (additive)
    let current = get_bond_record(env, signer);

    let new_amount = if let Some(existing) = current {
        existing.amount + amount  // Add to existing, don't replace
    } else {
        amount
    };

    let record = BondRecord {
        signer: signer.clone(),
        amount: new_amount,
        timestamp: env.ledger().timestamp(),
    };

    let key = (symbol_short!("bond"), signer);
    env.storage().persistent().set(&key, &record);
}

pub fn get_unbond_request(env: &Env, signer: &Address) -> Option<UnbondRequest> {
    let key = (symbol_short!("unbond"), signer);
    env.storage().persistent().get(&key)
}

pub fn unbond_collateral(env: &Env, signer: &Address, amount: i128) -> Result<(), Symbol> {
    let current = get_bond_record(env, signer)
        .ok_or(symbol_short!("nobond"))?;

    if current.amount < amount {
        return Err(symbol_short!("insuf"));
    }

    // Create unbond request with delay (e.g., 24 hours)
    let request = UnbondRequest {
        signer: signer.clone(),
        amount,
        requested_at: env.ledger().timestamp(),
        available_at: env.ledger().timestamp() + 86400,
    };

    let key = (symbol_short!("unbond"), signer);
    env.storage().persistent().set(&key, &request);

    Ok(())
}

pub fn claim_unbond(env: &Env, signer: &Address) -> Result<i128, Symbol> {
    let request = get_unbond_request(env, signer)
        .ok_or(symbol_short!("nounbond"))?;

    if env.ledger().timestamp() < request.available_at {
        return Err(symbol_short!("tooearly"));
    }

    // Remove unbond request
    let key = (symbol_short!("unbond"), signer);
    env.storage().persistent().remove(&key);

    Ok(request.amount)
}
