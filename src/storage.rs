use soroban_sdk::{contracttype, Address, Env, Vec};

use crate::types::{DisputeRecord, Transaction, TransactionStatus};

/// Storage key for the transaction counter.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    TxCount,
    Tx(u64),
    Dispute(u64),
    DisputeQueue,
}

/// Returns the next transaction id and increments the counter.
pub fn next_tx_id(env: &Env) -> u64 {
    let id: u64 = env.storage().instance().get(&DataKey::TxCount).unwrap_or(0);
    env.storage().instance().set(&DataKey::TxCount, &(id + 1));
    id
}

/// Persists a transaction by id.
pub fn save_tx(env: &Env, tx: &Transaction) {
    env.storage().persistent().set(&DataKey::Tx(tx.id), tx);
}

/// Loads a transaction by id, if present.
pub fn load_tx(env: &Env, id: u64) -> Option<Transaction> {
    env.storage().persistent().get(&DataKey::Tx(id))
}

/// Returns true when `tx` has been merged away into another transaction.
///
/// Merged-away transactions remain in storage (issue #66 "keeps data"), but
/// they are excluded from every status-based bulk query so aggregates and
/// dashboards never double-count a duplicate alongside its primary. They
/// remain directly queryable by `tx_id` via [`load_tx`].
pub fn is_merged_away(tx: &Transaction) -> bool {
    tx.merged_into.is_some()
}

/// Returns all transactions currently in `status`, excluding merged-away
/// records.
///
/// Decision (issue #302): merged-away transactions are excluded from every
/// status-based bulk query but remain directly queryable by `tx_id`.
pub fn get_transactions_by_status(env: &Env, status: TransactionStatus) -> Vec<Transaction> {
    let count: u64 = env.storage().instance().get(&DataKey::TxCount).unwrap_or(0);
    let mut out = Vec::new(env);
    let mut i: u64 = 0;
    while i < count {
        if let Some(tx) = load_tx(env, i) {
            if tx.status == status && !is_merged_away(&tx) {
                out.push_back(tx);
            }
        }
        i += 1;
    }
    out
}

/// Returns the dispute queue, excluding disputes whose transaction has been
/// merged away.
///
/// Decision (issue #302): a merged-away transaction is excluded from the
/// dispute queue so a duplicate never appears alongside its primary. The
/// dispute record itself is preserved in storage and remains directly
/// queryable by `tx_id`.
pub fn get_dispute_queue(env: &Env) -> Vec<DisputeRecord> {
    let queue: Vec<u64> = env
        .storage()
        .persistent()
        .get(&DataKey::DisputeQueue)
        .unwrap_or(Vec::new(env));
    let mut out = Vec::new(env);
    for tx_id in queue.iter() {
        if let Some(record) = env
            .storage()
            .persistent()
            .get::<DataKey, DisputeRecord>(&DataKey::Dispute(tx_id))
        {
            let merged_away = load_tx(env, tx_id)
                .map(|tx| is_merged_away(&tx))
                .unwrap_or(false);
            if !merged_away {
                out.push_back(record);
            }
        }
    }
    out
}

/// Returns the transaction that `tx_id` was merged into, if any.
///
/// Round-trips the link written by `merge_duplicate_transactions` (issue #66)
/// so a merged-away transaction can always be resolved to its primary, even
/// when it was merged before other bulk queries existed.
pub fn get_merged_into(env: &Env, tx_id: u64) -> Option<u64> {
    load_tx(env, tx_id).and_then(|tx| tx.merged_into)
}

/// Returns the forwarding route stored on `tx_id`, if any.
///
/// Decision (issue #303): the forwarding route is orthogonal metadata on the
/// transaction record. It is never cleared or rewritten by a status
/// transition (Pending -> Processing -> Completed/Failed/Cancelled), so a
/// future Swap Engine polling `get_forwarding_route` always observes the same
/// route regardless of when it polls. When a transaction is merged away
/// (issue #66) the route is *dropped* from the merged-away record's read
/// surface: callers resolving through [`get_merged_into`] to the primary
/// observe the primary's own route, and the merged-away record's route is not
/// transferred. This keeps a single authoritative route per logical
/// transaction and avoids two records advertising conflicting routes.
pub fn get_forwarding_route(env: &Env, tx_id: u64) -> Option<Address> {
    load_tx(env, tx_id).and_then(|tx| {
        if is_merged_away(&tx) {
            None
        } else {
            tx.forwarding_route
        }
    })
}
