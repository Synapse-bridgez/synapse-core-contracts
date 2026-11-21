// Issues #248, #247, #246, #245 - Final Event Restoration

#![no_std]

use soroban_sdk::{Env, Address, Symbol, symbol_short, Vec};

// ============================================================================
// Issue #248: Restore EventParamSet
// ============================================================================

pub fn emit_param_set(env: &Env, param_key: &Symbol, param_value: &[u8]) {
    env.events().publish(
        (symbol_short!("param"), symbol_short!("set")),
        (param_key, param_value),
    );
}

pub fn set_registry_param(env: &Env, key: &Symbol, value: &[u8]) {
    // FIX: Emit on every write per commit 47dac6a spec
    env.storage()
        .persistent()
        .set(&(symbol_short!("param"), key), &value);

    emit_param_set(env, key, value);
}

// ============================================================================
// Issue #247: Restore EventBatchProcessed
// ============================================================================

pub fn emit_batch_processed(
    env: &Env,
    batch_id: u32,
    total_count: u32,
    // FIX: Reuse IDs from validation, don't re-decode
    tx_ids: &Vec<Symbol>,
) {
    env.events().publish(
        (symbol_short!("batch"), symbol_short!("process")),
        (batch_id, total_count, tx_ids),
    );
}

pub fn process_batch(
    env: &Env,
    batch_id: u32,
    payloads: &[Vec<u8>],
) {
    // During validation, collect IDs
    let mut tx_ids = Vec::new();

    for payload in payloads {
        // Validate and extract ID (once)
        if let Some(id) = extract_tx_id(payload) {
            tx_ids.push_back(id);
        }
    }

    // FIX: Emit using collected IDs (not re-decoding)
    emit_batch_processed(env, batch_id, payloads.len() as u32, &tx_ids);
}

fn extract_tx_id(payload: &[u8]) -> Option<Symbol> {
    // Extract TX ID from payload (simplified)
    Some(symbol_short!("tx"))
}

// ============================================================================
// Issue #246: Restore EventDisputeRaised
// ============================================================================

pub fn emit_dispute_raised(env: &Env, tx_id: &Symbol, disputer: &Address, reason: &Symbol) {
    env.events().publish(
        (symbol_short!("dispute"), symbol_short!("raise")),
        (tx_id, disputer, reason),
    );
}

pub fn raise_dispute(env: &Env, tx_id: &Symbol, disputer: &Address, reason: &Symbol) {
    // FIX: Emit event (restore EventDisputeRaised)
    emit_dispute_raised(env, tx_id, disputer, reason);

    // Store dispute record
    env.storage()
        .persistent()
        .set(&(symbol_short!("dispute"), tx_id), &reason);
}

// ============================================================================
// Issue #245: Restore EventTransactionRetried
// ============================================================================

pub fn emit_transaction_retried(env: &Env, tx_id: &Symbol, retry_count: u32) {
    // FIX: Emit with POST-INCREMENT retry_count
    env.events().publish(
        (symbol_short!("txn"), symbol_short!("retry")),
        (tx_id, retry_count),
    );
}

pub fn retry_transaction(env: &Env, tx_id: &Symbol) -> Result<u32, Symbol> {
    // Get current retry count
    let current: u32 = env.storage()
        .persistent()
        .get(&(symbol_short!("retry"), tx_id))
        .unwrap_or(0);

    // Increment
    let new_count = current + 1;

    // Check max retries (e.g., 3)
    if new_count > 3 {
        return Err(symbol_short!("maxretries"));
    }

    // Store new count
    env.storage()
        .persistent()
        .set(&(symbol_short!("retry"), tx_id), &new_count);

    // FIX: Emit with POST-INCREMENT value (new_count)
    emit_transaction_retried(env, tx_id, new_count);

    Ok(new_count)
}
