//! Ledger storage helpers for the transaction contract.
//!
//! This module owns the pack/unpack conversion between the public
//! [`Transaction`] type and the packed [`StoredTransaction`] form persisted to
//! ledger storage, and also centralises the storage keys and small read/write
//! helpers used by the contract entry points.

use crate::types::{CallbackType, StoredTransaction, Transaction, TransactionStatus};
use soroban_sdk::{contracttype, Address, Env};

/// Bit offset of the `status` field within `StoredTransaction::packed`.
const STATUS_SHIFT: u32 = 0;
/// Bit mask for the `status` field (4 bits).
const STATUS_MASK: u32 = 0xF;

/// Bit offset of the `callback_type` field within `StoredTransaction::packed`.
const CALLBACK_SHIFT: u32 = 4;
/// Bit mask for the `callback_type` field (4 bits).
const CALLBACK_MASK: u32 = 0xF;

/// Bit offset of the `retry_count` field within `StoredTransaction::packed`.
const RETRY_SHIFT: u32 = 8;
/// Bit mask for the `retry_count` field (4 bits).
///
/// Sized generously (4 bits, max 15) relative to the original wave 9 design
/// intent of a max-retry constant of 3, so a future increase does not require
/// a breaking re-pack of the stored representation.
const RETRY_MASK: u32 = 0xF;

/// Storage keys used by the contract.
///
/// Kept as a single enum so that every entry point shares one namespaced key
/// space and later issues can extend it without touching `lib.rs`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Contract administrator address.
    Admin,
    /// Relay signer address authorised to submit relayed operations.
    RelaySigner,
    /// Whether the contract has been initialised.
    Initialized,
}

/// Map a `u32` discriminant back to a [`TransactionStatus`].
fn status_from_u32(value: u32) -> TransactionStatus {
    match value {
        1 => TransactionStatus::Completed,
        2 => TransactionStatus::Failed,
        3 => TransactionStatus::Retrying,
        4 => TransactionStatus::Cancelled,
        _ => TransactionStatus::Pending,
    }
}

/// Map a `u32` discriminant back to a [`CallbackType`].
fn callback_from_u32(value: u32) -> CallbackType {
    match value {
        1 => CallbackType::OnSuccess,
        2 => CallbackType::OnFailure,
        3 => CallbackType::OnComplete,
        _ => CallbackType::None,
    }
}

/// Pack a [`Transaction`] into its on-ledger [`StoredTransaction`] form.
pub fn pack(tx: &Transaction) -> StoredTransaction {
    let packed = ((tx.status as u32 & STATUS_MASK) << STATUS_SHIFT)
        | ((tx.callback_type as u32 & CALLBACK_MASK) << CALLBACK_SHIFT)
        | ((tx.retry_count & RETRY_MASK) << RETRY_SHIFT);

    StoredTransaction {
        tx_id: tx.tx_id.clone(),
        packed,
        amount: tx.amount,
        stellar_account: tx.stellar_account.clone(),
        asset_code: tx.asset_code.clone(),
        asset_issuer: tx.asset_issuer.clone(),
        tags: tx.tags.clone(),
    }
}

/// Unpack a [`StoredTransaction`] back into the public [`Transaction`] form.
pub fn unpack(stored: &StoredTransaction) -> Transaction {
    let status = status_from_u32((stored.packed >> STATUS_SHIFT) & STATUS_MASK);
    let callback_type = callback_from_u32((stored.packed >> CALLBACK_SHIFT) & CALLBACK_MASK);
    let retry_count = (stored.packed >> RETRY_SHIFT) & RETRY_MASK;

    Transaction {
        tx_id: stored.tx_id.clone(),
        status,
        callback_type,
        amount: stored.amount,
        stellar_account: stored.stellar_account.clone(),
        asset_code: stored.asset_code.clone(),
        asset_issuer: stored.asset_issuer.clone(),
        retry_count,
        tags: stored.tags.clone(),
    }
}

/// Returns whether the contract has been initialised.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
///
/// # Returns
///
/// `true` if the `Initialized` flag is set in instance storage.
#[must_use]
pub fn is_initialized(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Initialized)
        .unwrap_or(false)
}

/// Marks the contract as initialised in instance storage.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
pub fn set_initialized(env: &Env) {
    env.storage()
        .instance()
        .set(&DataKey::Initialized, &true);
}

/// Stores the contract administrator address in instance storage.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
/// * `admin` - The administrator address to persist.
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

/// Returns the stored contract administrator address, if any.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
///
/// # Returns
///
/// `Some(admin)` when the contract has been initialised, otherwise `None`.
#[must_use]
pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

/// Stores the relay signer address in instance storage.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
/// * `relay_signer` - The relay signer address to persist.
pub fn set_relay_signer(env: &Env, relay_signer: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::RelaySigner, relay_signer);
}

/// Returns the stored relay signer address, if any.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
///
/// # Returns
///
/// `Some(relay_signer)` when the contract has been initialised, otherwise `None`.
#[must_use]
pub fn get_relay_signer(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::RelaySigner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{Address, Bytes, Env, String, Vec};

    #[test]
    fn round_trip_preserves_every_field() {
        let env = Env::default();
        let tx = Transaction {
            tx_id: Bytes::from_array(&env, &[1, 2, 3, 4]),
            status: TransactionStatus::Retrying,
            callback_type: CallbackType::OnFailure,
            amount: 1_000_000,
            stellar_account: Address::generate(&env),
            asset_code: String::from_str(&env, "USDC"),
            asset_issuer: Address::generate(&env),
            retry_count: 3,
            tags: Vec::new(&env),
        };

        let stored = pack(&tx);
        let restored = unpack(&stored);

        assert_eq!(restored, tx);
    }

    #[test]
    fn retry_count_defaults_to_zero_and_round_trips() {
        let env = Env::default();
        let tx = Transaction {
            tx_id: Bytes::from_array(&env, &[9, 9]),
            status: TransactionStatus::Pending,
            callback_type: CallbackType::None,
            amount: 0,
            stellar_account: Address::generate(&env),
            asset_code: String::from_str(&env, "XLM"),
            asset_issuer: Address::generate(&env),
            retry_count: 0,
            tags: Vec::new(&env),
        };

        let stored = pack(&tx);
        assert_eq!((stored.packed >> RETRY_SHIFT) & RETRY_MASK, 0);
        assert_eq!(unpack(&stored).retry_count, 0);
    }

    #[test]
    fn retry_count_is_masked_to_four_bits() {
        let env = Env::default();
        let tx = Transaction {
            tx_id: Bytes::from_array(&env, &[7]),
            status: TransactionStatus::Retrying,
            callback_type: CallbackType::OnComplete,
            amount: 5,
            stellar_account: Address::generate(&env),
            asset_code: String::from_str(&env, "USDC"),
            asset_issuer: Address::generate(&env),
            retry_count: 0xF,
            tags: Vec::new(&env),
        };

        let stored = pack(&tx);
        assert_eq!((stored.packed >> RETRY_SHIFT) & RETRY_MASK, 0xF);
        assert_eq!(unpack(&stored).retry_count, 0xF);
    }
}
}
