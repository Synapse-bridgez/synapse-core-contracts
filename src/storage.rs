use soroban_sdk::{contracttype, Address, BytesN, Env, Vec};

use crate::types::{ContractError, Transaction, TransactionStatus};

/// Storage key namespace for transaction records.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Full transaction record keyed by its id.
    Transaction(BytesN<32>),
    /// Narrow status-only record keyed by its id (cheap read path).
    TransactionStatus(BytesN<32>),
    /// Redirect from a merged-away tx id to its surviving tx id.
    MergedInto(BytesN<32>),
}

/// Persist a transaction record and its status index.
pub fn save_transaction(env: &Env, tx: &Transaction) {
    env.storage()
        .persistent()
        .set(&DataKey::Transaction(tx.id.clone()), tx);
    env.storage()
        .persistent()
        .set(&DataKey::TransactionStatus(tx.id.clone()), &tx.status);
}

/// Record that `merged_id` was merged into `surviving_id`.
pub fn set_merged_into(env: &Env, merged_id: &BytesN<32>, surviving_id: &BytesN<32>) {
    env.storage()
        .persistent()
        .set(&DataKey::MergedInto(merged_id.clone()), surviving_id);
}

/// Resolve a possibly-merged tx id to the id holding the live record.
fn resolve_id(env: &Env, tx_id: &BytesN<32>) -> BytesN<32> {
    env.storage()
        .persistent()
        .get::<_, BytesN<32>>(&DataKey::MergedInto(tx_id.clone()))
        .unwrap_or_else(|| tx_id.clone())
}

/// Read a full transaction record by id.
///
/// Merged-away ids resolve to the surviving record so callers can still
/// query by the original `tx_id` (see `test_recovery.rs`).
pub fn get_transaction(env: &Env, tx_id: &BytesN<32>) -> Result<Transaction, ContractError> {
    let resolved = resolve_id(env, tx_id);
    env.storage()
        .persistent()
        .get::<_, Transaction>(&DataKey::Transaction(resolved))
        .ok_or(ContractError::TransactionNotFound)
}

/// Read only the status field for a transaction id.
///
/// This is a narrow storage read of the status index rather than a full
/// `get_transaction` call, keeping the entry point cheap per `COST_MODEL.md`.
pub fn get_status(env: &Env, tx_id: &BytesN<32>) -> Result<TransactionStatus, ContractError> {
    let resolved = resolve_id(env, tx_id);
    env.storage()
        .persistent()
        .get::<_, TransactionStatus>(&DataKey::TransactionStatus(resolved))
        .ok_or(ContractError::TransactionNotFound)
}
