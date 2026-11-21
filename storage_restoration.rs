// src/storage.rs - Issues #233, #235, #234, #236
// Restore StorageClient persistent/instance/temporary storage helpers

#![no_std]

use soroban_sdk::{Env, Address, Symbol, symbol_short, contracttype};

// ============================================================================
// Issue #233: Persistent Storage for Transaction Records
// ============================================================================

#[derive(Clone)]
#[contracttype]
pub struct StoredTransaction {
    pub transaction_id: Symbol,
    pub status: u32,  // 0=registered, 1=executed, 2=failed, 3=cancelled
    pub timestamp: u64,
    pub payload_hash: [u8; 32],
}

pub fn store_transaction(env: &Env, tx_id: &Symbol, tx: &StoredTransaction) {
    let key = (symbol_short!("tx"), tx_id);
    env.storage().persistent().set(&key, tx);
}

pub fn get_transaction(env: &Env, tx_id: &Symbol) -> Option<StoredTransaction> {
    let key = (symbol_short!("tx"), tx_id);
    env.storage().persistent().get(&key)
}

pub fn has_transaction(env: &Env, tx_id: &Symbol) -> bool {
    get_transaction(env, tx_id).is_some()
}

// ============================================================================
// Issue #235: Instance Storage for Hot-Path Values
// ============================================================================

#[derive(Clone)]
#[contracttype]
pub struct ContractState {
    pub admin: Address,
    pub relay_signer: Address,
    pub paused: bool,
    pub default_ceiling: i128,
}

pub fn init_contract_state(env: &Env, state: &ContractState) {
    env.storage().instance().set(&symbol_short!("state"), state);
}

pub fn get_contract_state(env: &Env) -> Option<ContractState> {
    env.storage().instance().get(&symbol_short!("state"))
}

pub fn set_paused(env: &Env, paused: bool) {
    let mut state = get_contract_state(env).expect("state not initialized");
    state.paused = paused;
    env.storage().instance().set(&symbol_short!("state"), &state);
}

pub fn is_paused(env: &Env) -> bool {
    get_contract_state(env).map(|s| s.paused).unwrap_or(false)
}

pub fn get_default_ceiling(env: &Env) -> i128 {
    get_contract_state(env).map(|s| s.default_ceiling).unwrap_or(0)
}

pub fn get_relay_signer(env: &Env) -> Option<Address> {
    get_contract_state(env).map(|s| s.relay_signer)
}

pub fn get_admin(env: &Env) -> Option<Address> {
    get_contract_state(env).map(|s| s.admin)
}

// ============================================================================
// Issue #234: Temporary Storage for Idempotency Keys (24h TTL)
// ============================================================================

pub fn store_idempotency_key(env: &Env, key: &Symbol, tx_id: &Symbol) {
    let storage_key = (symbol_short!("idempot"), key);
    env.storage().temporary().set(&storage_key, tx_id);
}

pub fn get_idempotency_result(env: &Env, key: &Symbol) -> Option<Symbol> {
    let storage_key = (symbol_short!("idempot"), key);
    env.storage().temporary().get(&storage_key)
}

pub fn idempotency_key_exists(env: &Env, key: &Symbol) -> bool {
    get_idempotency_result(env, key).is_some()
}

// ============================================================================
// Issue #234: Durable Secondary Guard (Permanent Hard Guard)
// ============================================================================

pub fn store_transaction_durable(env: &Env, tx_id: &Symbol, timestamp: u64) {
    let key = (symbol_short!("txdurable"), tx_id);
    env.storage().persistent().set(&key, &timestamp);
}

pub fn has_transaction_durable(env: &Env, tx_id: &Symbol) -> bool {
    let key = (symbol_short!("txdurable"), tx_id);
    env.storage().persistent().has(&key)
}

// ============================================================================
// Issue #236: O(1) Slot-Based Status Index
// ============================================================================

#[derive(Clone)]
#[contracttype]
pub struct StatusSlot {
    pub slot_index: u32,
    pub transaction_ids: [Symbol; 1024],  // Fixed-size array for O(1) access
    pub count: u32,
}

pub fn get_status_index(env: &Env, status: u32) -> Option<StatusSlot> {
    let key = (symbol_short!("statidx"), status);
    env.storage().persistent().get(&key)
}

pub fn add_to_status_index(env: &Env, status: u32, tx_id: Symbol) {
    let key = (symbol_short!("statidx"), status);
    let mut index = env.storage().persistent().get(&key).unwrap_or(StatusSlot {
        slot_index: 0,
        transaction_ids: [symbol_short!(""); 1024],
        count: 0,
    });

    if index.count < 1024 {
        index.transaction_ids[index.count as usize] = tx_id;
        index.count += 1;
        env.storage().persistent().set(&key, &index);
    }
}

pub fn get_transactions_by_status(env: &Env, status: u32, offset: u32, limit: u32) -> Vec<Symbol> {
    let key = (symbol_short!("statidx"), status);
    let index = match env.storage().persistent().get(&key) {
        Some(idx) => idx,
        None => return soroban_sdk::vec![env],
    };

    let mut result = soroban_sdk::vec![env];
    let end = std::cmp::min(offset + limit, index.count);

    for i in offset..end {
        result.push_back(index.transaction_ids[i as usize].clone());
    }

    result
}

pub fn update_transaction_status(env: &Env, tx_id: &Symbol, old_status: u32, new_status: u32) {
    // Remove from old status index
    let old_key = (symbol_short!("statidx"), old_status);
    if let Some(mut index) = env.storage().persistent().get::<_, StatusSlot>(&old_key) {
        // Find and remove tx_id
        for i in 0..index.count as usize {
            if index.transaction_ids[i] == *tx_id {
                // Swap with last element
                index.transaction_ids[i] = index.transaction_ids[index.count as usize - 1];
                index.count -= 1;
                break;
            }
        }
        env.storage().persistent().set(&old_key, &index);
    }

    // Add to new status index
    add_to_status_index(env, new_status, tx_id.clone());
}
