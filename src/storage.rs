//! # Storage
//!
//! All ledger read/write operations are centralised here, keeping handler and
//! service logic free of raw `env.storage()` calls.
//!
//! ## Storage tiers used
//!
//! | Data                  | Tier       | Rationale                                  |
//! |-----------------------|------------|--------------------------------------------|
//! | Admin, relay signer   | `persistent` | Must survive archive/restore cycles      |
//! | Transactions          | `persistent` | Long-lived; needed for audit trail       |
//! | Idempotency keys      | `temporary`  | 24-hour TTL; evicted by the ledger       |
//! | Initialised flag      | `instance`   | Lives with the contract instance         |

use soroban_sdk::{Address, Env, String};

use crate::types::{ContractError, StorageKey, Transaction};

/// TTL extension in ledgers applied to idempotency keys (~24 hours at ~5s/ledger).
///
/// 24 * 3600 / 5 = 17_280 ledgers.  We round up to 18_000 for safety.
const IDEMPOTENCY_TTL_LEDGERS: u32 = 18_000;

/// Minimum TTL we require on transaction records before extending.
const TRANSACTION_MIN_TTL_LEDGERS: u32 = 100_000; // ~1 week

pub struct StorageClient;

impl StorageClient {
    // ── Initialisation flag ───────────────────────────────────────────────────

    /// Returns `true` if [`crate::SynapseCoreContract::initialize`] has been called.
    pub fn is_initialised(env: &Env) -> bool {
        env.storage()
            .instance()
            .has(&StorageKey::Initialised)
    }

    /// Persist the initialised flag.  Called exactly once during `initialize()`.
    pub fn set_initialised(env: &Env) {
        // TODO: env.storage().instance().set(&StorageKey::Initialised, &true)
        todo!()
    }

    // ── Admin ─────────────────────────────────────────────────────────────────

    /// Read the current admin address from persistent storage.
    pub fn get_admin(env: &Env) -> Result<Address, ContractError> {
        // TODO: env.storage().persistent().get(&StorageKey::Admin)
        //       .ok_or(ContractError::NotInitialised)
        todo!()
    }

    /// Persist an admin address.
    pub fn set_admin(env: &Env, admin: &Address) {
        // TODO: env.storage().persistent().set(&StorageKey::Admin, admin)
        todo!()
    }

    // ── Relay signer ──────────────────────────────────────────────────────────

    /// Read the trusted relay signer address.
    pub fn get_relay_signer(env: &Env) -> Result<Address, ContractError> {
        // TODO: env.storage().persistent().get(&StorageKey::RelaySigner)
        //       .ok_or(ContractError::NotInitialised)
        todo!()
    }

    /// Persist the relay signer address.
    pub fn set_relay_signer(env: &Env, signer: &Address) {
        // TODO: env.storage().persistent().set(&StorageKey::RelaySigner, signer)
        todo!()
    }

    // ── Transactions ──────────────────────────────────────────────────────────

    /// Read a [`Transaction`] by its ID.
    ///
    /// Extends the ledger TTL on each access so active records are never evicted.
    pub fn get_transaction(env: &Env, tx_id: &String) -> Result<Transaction, ContractError> {
        // TODO: let key = StorageKey::Transaction(tx_id.clone());
        // TODO: env.storage()
        //           .persistent()
        //           .get::<StorageKey, Transaction>(&key)
        //           .ok_or(ContractError::TransactionNotFound)
        //
        // After retrieval, extend TTL:
        // TODO: env.storage().persistent().extend_ttl(&key, TRANSACTION_MIN_TTL_LEDGERS, ...)
        todo!()
    }

    /// Persist (insert or update) a [`Transaction`].
    pub fn save_transaction(env: &Env, tx: &Transaction) {
        // TODO: let key = StorageKey::Transaction(tx.id.clone());
        // TODO: env.storage().persistent().set(&key, tx)
        // TODO: extend TTL on the key
        todo!()
    }

    // ── Idempotency keys ──────────────────────────────────────────────────────

    /// Return the ledger sequence at which an idempotency key was first stored,
    /// or `None` if the key is unknown / expired.
    pub fn get_idempotency_key(env: &Env, key: &String) -> Option<u32> {
        // TODO: env.storage()
        //           .temporary()
        //           .get::<StorageKey, u32>(&StorageKey::IdempotencyKey(key.clone()))
        todo!()
    }

    /// Record an idempotency key with a ~24-hour TTL.
    pub fn set_idempotency_key(env: &Env, key: &String) {
        // TODO: env.storage()
        //           .temporary()
        //           .set(&StorageKey::IdempotencyKey(key.clone()),
        //                &env.ledger().sequence())
        // TODO: env.storage().temporary().extend_ttl(
        //           &StorageKey::IdempotencyKey(key.clone()),
        //           IDEMPOTENCY_TTL_LEDGERS,
        //           IDEMPOTENCY_TTL_LEDGERS,
        //       )
        todo!()
    }
}
