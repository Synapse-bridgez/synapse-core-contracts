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
    /// Temporary idempotency key recorded by `register_callback` (issue #22).
    IdempotencyKey(BytesN<32>),
    /// Current accrued-and-unwithdrawn treasury balance (issue #30).
    TreasuryBalance,
    /// Treasury configuration: fee bps, epoch cap, epoch length (issue #30).
    TreasuryConfig,
    /// In-flight two-party withdrawal proposal, if any (issue #72).
    PendingWithdrawal,
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

/// Check whether an idempotency key is currently recorded (issue #22).
///
/// This is a pure read of the temporary idempotency-key entry: it never
/// writes, never creates a key that did not already exist, and never extends
/// the key's TTL. Callers can therefore probe idempotency state before
/// submitting a `register_callback` call without side effects.
pub fn is_duplicate(env: &Env, key: &BytesN<32>) -> bool {
    env.storage()
        .temporary()
        .has(&DataKey::IdempotencyKey(key.clone()))
}

/// Read the current accrued-and-unwithdrawn treasury balance (issue #291).
///
/// Pure passthrough read of the balance written by the fee accrual path
/// (issue #30). Returns `0` when nothing has accrued yet.
pub fn get_treasury_balance(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get::<_, i128>(&DataKey::TreasuryBalance)
        .unwrap_or(0)
}

/// Read the treasury configuration (issue #291).
///
/// Pure passthrough read of the config written by the treasury setup path
/// (issue #30). Returns `None` when the treasury has not been configured.
pub fn get_treasury_config(env: &Env) -> Option<crate::types::TreasuryConfig> {
    env.storage()
        .persistent()
        .get::<_, crate::types::TreasuryConfig>(&DataKey::TreasuryConfig)
}

/// Read the current in-flight two-party withdrawal proposal (issue #291).
///
/// Pure passthrough read of the proposal written by the withdrawal flow
/// (issue #72). Returns `None` cleanly when there is no in-flight proposal.
pub fn get_pending_withdrawal(env: &Env) -> Option<crate::types::Withdrawal> {
    env.storage()
        .persistent()
        .get::<_, crate::types::Withdrawal>(&DataKey::PendingWithdrawal)
}
