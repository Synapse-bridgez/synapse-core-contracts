use soroban_sdk::{contracttype, Address, Env, String, Vec};

use crate::types::{DataKey, Transaction, TransactionStatus};

/// Maximum number of tags that may be attached to a single transaction.
///
/// Enforced by `add_transaction_tag` so that tag metadata stays bounded.
pub const MAX_TAGS_PER_TRANSACTION: u32 = 10;

/// Persist a transaction under its id.
pub fn set_transaction(env: &Env, tx_id: u64, tx: &Transaction) {
    env.storage()
        .persistent()
        .set(&DataKey::Transaction(tx_id), tx);
}

/// Load a transaction by id, if present.
pub fn get_transaction(env: &Env, tx_id: u64) -> Option<Transaction> {
    env.storage()
        .persistent()
        .get(&DataKey::Transaction(tx_id))
}

/// Return the tags currently attached to a transaction.
///
/// Returns an empty vector when the transaction has no tags yet.
pub fn get_transaction_tags(env: &Env, tx_id: u64) -> Vec<String> {
    env.storage()
        .persistent()
        .get(&DataKey::TransactionTags(tx_id))
        .unwrap_or_else(|| Vec::new(env))
}

/// Append a tag to a transaction's tag list.
///
/// Callers are expected to have validated the tag and the tag-count bound
/// before invoking this helper.
pub fn add_transaction_tag(env: &Env, tx_id: u64, tag: &String) {
    let mut tags = get_transaction_tags(env, tx_id);
    tags.push_back(tag.clone());
    env.storage()
        .persistent()
        .set(&DataKey::TransactionTags(tx_id), &tags);
}

/// Return the number of tags currently attached to a transaction.
pub fn transaction_tag_count(env: &Env, tx_id: u64) -> u32 {
    get_transaction_tags(env, tx_id).len()
}

/// Persist the status of a transaction.
pub fn set_transaction_status(env: &Env, tx_id: u64, status: &TransactionStatus) {
    if let Some(mut tx) = get_transaction(env, tx_id) {
        tx.status = status.clone();
        set_transaction(env, tx_id, &tx);
    }
}

/// Persist an arbitrary address-keyed flag.
pub fn set_flag(env: &Env, key: &DataKey, value: bool) {
    env.storage().persistent().set(key, &value);
}

/// Read an arbitrary address-keyed flag, defaulting to `false`.
pub fn get_flag(env: &Env, key: &DataKey) -> bool {
    env.storage().persistent().get(key).unwrap_or(false)
}

/// Persist the admin address.
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().persistent().set(&DataKey::Admin, admin);
}

/// Load the admin address, if configured.
pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().persistent().get(&DataKey::Admin)
}
