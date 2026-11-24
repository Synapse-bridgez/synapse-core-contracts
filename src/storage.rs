//! Storage helpers.
//!
//! Singleton config (admin, relay signer, pending admin, guardian, pause flag
//! and reason, initialisation flag, treasury balance) lives in instance
//! storage; per-transaction records and tag lists live in persistent storage
//! keyed by transaction id.
//!
//! `add_transaction_tag` enforces `MAX_TAGS_PER_TRANSACTION` (issue #471): on
//! overflow it emits a `taglimit` event carrying the supplied count and the
//! limit, then returns `ContractError::TagLimitExceeded`.

use soroban_sdk::{Address, Env, String, Vec};

use crate::events::emit_tag_limit_exceeded;
use crate::types::{ContractError, DataKey, Transaction, MAX_TAGS_PER_TRANSACTION};

/// Read the admin address, if set.
pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

/// Store the admin address.
pub fn set_admin(env: &Env, admin: Address) {
    env.storage().instance().set(&DataKey::Admin, &admin);
}

/// Read the relay signer, if set.
pub fn get_relay_signer(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::RelaySigner)
}

/// Store the relay signer.
pub fn set_relay_signer(env: &Env, relay_signer: Address) {
    env.storage()
        .instance()
        .set(&DataKey::RelaySigner, &relay_signer);
}

/// Read the pending admin, if set.
pub fn get_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::PendingAdmin)
}

/// Store the pending admin.
pub fn set_pending_admin(env: &Env, pending: Address) {
    env.storage()
        .instance()
        .set(&DataKey::PendingAdmin, &pending);
}

/// Clear the pending admin.
pub fn clear_pending_admin(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingAdmin);
}

/// Read the guardian, if set.
pub fn get_guardian(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Guardian)
}

/// Store the guardian.
pub fn set_guardian(env: &Env, guardian: Address) {
    env.storage().instance().set(&DataKey::Guardian, &guardian);
}

/// Whether the contract has been initialised.
pub fn is_initialised(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Initialised)
}

/// Mark the contract initialised.
pub fn set_initialised(env: &Env) {
    env.storage().instance().set(&DataKey::Initialised, &true);
}

/// Whether the contract is paused (default unpaused).
pub fn is_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

/// Set the pause flag.
pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::Paused, &paused);
}

/// Read the pause reason, if any (absent means no reason recorded).
pub fn get_pause_reason(env: &Env) -> Option<String> {
    env.storage().instance().get(&DataKey::PauseReason)
}

/// Record the pause reason.
pub fn set_pause_reason(env: &Env, reason: String) {
    env.storage().instance().set(&DataKey::PauseReason, &reason);
}

/// Clear any recorded pause reason.
pub fn clear_pause_reason(env: &Env) {
    if env.storage().instance().has(&DataKey::PauseReason) {
        env.storage().instance().remove(&DataKey::PauseReason);
    }
}

/// Read the treasury balance (default 0).
pub fn get_treasury_balance(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::TreasuryBalance)
        .unwrap_or(0)
}

/// Store the treasury balance.
pub fn set_treasury_balance(env: &Env, balance: i128) {
    env.storage()
        .instance()
        .set(&DataKey::TreasuryBalance, &balance);
}

/// Persist a transaction.
pub fn save_transaction(env: &Env, tx: Transaction) {
    env.storage()
        .persistent()
        .set(&DataKey::Transaction(tx.id), &tx);
}

/// Whether a transaction exists.
pub fn transaction_exists(env: &Env, tx_id: u64) -> bool {
    env.storage().persistent().has(&DataKey::Transaction(tx_id))
}

/// Read the tags attached to a transaction (empty when none).
pub fn get_transaction_tags(env: &Env, tx_id: u64) -> Vec<String> {
    env.storage()
        .persistent()
        .get(&DataKey::TransactionTags(tx_id))
        .unwrap_or_else(|| Vec::new(env))
}

/// Append a tag, enforcing `MAX_TAGS_PER_TRANSACTION` (issue #471).
///
/// Returns `ContractError::NotFound` when the transaction does not exist. On
/// overflow this emits a `taglimit` event carrying the supplied count and the
/// limit, then returns `ContractError::TagLimitExceeded`.
pub fn add_transaction_tag(env: &Env, tx_id: u64, tag: String) -> Result<(), ContractError> {
    if !transaction_exists(env, tx_id) {
        return Err(ContractError::NotFound);
    }
    let tags = get_transaction_tags(env, tx_id);
    let supplied = tags.len();
    if supplied >= MAX_TAGS_PER_TRANSACTION {
        emit_tag_limit_exceeded(env, supplied + 1, MAX_TAGS_PER_TRANSACTION);
        return Err(ContractError::TagLimitExceeded);
    }
    let mut tags = tags;
    tags.push_back(tag);
    env.storage()
        .persistent()
        .set(&DataKey::TransactionTags(tx_id), &tags);
    Ok(())
}
