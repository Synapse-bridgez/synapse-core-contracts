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

use crate::types::{ContractError, RelaySignerSet, StorageKey, Transaction};

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
        env.storage().instance().has(&StorageKey::Initialised)
    }

    /// Persist the initialised flag.  Called exactly once during `initialize()`.
    pub fn set_initialised(env: &Env) {
        env.storage()
            .instance()
            .set(&StorageKey::Initialised, &true);
    }

    // ── Pause / circuit breaker ───────────────────────────────────────────────

    /// Returns `true` when the emergency-pause flag is engaged.
    ///
    /// Defaults to `false` when the flag has never been written, so a freshly
    /// initialised contract is always unpaused.
    pub fn is_paused(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&StorageKey::Paused)
            .unwrap_or(false)
    }

    /// Persist the emergency-pause flag.
    pub fn set_paused(env: &Env, paused: bool) {
        env.storage().instance().set(&StorageKey::Paused, &paused);
    }

    // ── Admin ─────────────────────────────────────────────────────────────────

    /// Read the current admin address from persistent storage.
    pub fn get_admin(env: &Env) -> Result<Address, ContractError> {
        env.storage()
            .persistent()
            .get(&StorageKey::Admin)
            .ok_or(ContractError::NotInitialised)
    }

    /// Persist an admin address.
    pub fn set_admin(env: &Env, admin: &Address) {
        env.storage().persistent().set(&StorageKey::Admin, admin);
    }

    // ── Relay signer ──────────────────────────────────────────────────────────

    /// Read the trusted relay signer address.
    ///
    /// With an N-of-M set this is the primary (first) signer.
    pub fn get_relay_signer(env: &Env) -> Result<Address, ContractError> {
        Self::get_relay_signer_set(env)?
            .signers
            .get(0)
            .ok_or(ContractError::NotInitialised)
    }

    /// Persist the relay signer address (replaces the primary signer when a
    /// signer set exists).
    pub fn set_relay_signer(env: &Env, signer: &Address) {
        env.storage()
            .persistent()
            .set(&StorageKey::RelaySigner, signer);
        if let Some(mut set) = Self::get_relay_signer_set_opt(env) {
            set.signers.set(0, signer.clone());
            Self::set_relay_signer_set(env, &set);
        }
    }

    fn get_relay_signer_set_opt(env: &Env) -> Option<RelaySignerSet> {
        env.storage().persistent().get(&StorageKey::RelaySignerSet)
    }

    /// Read the relay signer set. Pre-N-of-M deployments migrate lazily:
    /// the legacy single `RelaySigner` becomes `threshold = 1, signers = [it]`.
    pub fn get_relay_signer_set(env: &Env) -> Result<RelaySignerSet, ContractError> {
        if let Some(set) = Self::get_relay_signer_set_opt(env) {
            return Ok(set);
        }
        let legacy: Address = env
            .storage()
            .persistent()
            .get(&StorageKey::RelaySigner)
            .ok_or(ContractError::NotInitialised)?;
        Ok(RelaySignerSet {
            signers: soroban_sdk::vec![env, legacy],
            threshold: 1,
        })
    }

    /// Persist the relay signer set.
    pub fn set_relay_signer_set(env: &Env, set: &RelaySignerSet) {
        env.storage()
            .persistent()
            .set(&StorageKey::RelaySignerSet, set);
    }

    /// Record a signer's approval at the current ledger.
    pub fn set_relay_approval(env: &Env, signer: &Address) {
        env.storage().temporary().set(
            &StorageKey::RelayApproval(signer.clone()),
            &env.ledger().sequence(),
        );
    }

    /// Ledger at which `signer` last approved, if any.
    pub fn get_relay_approval(env: &Env, signer: &Address) -> Option<u32> {
        env.storage()
            .temporary()
            .get(&StorageKey::RelayApproval(signer.clone()))
    }

    /// Consume (remove) `signer`'s approval.
    pub fn clear_relay_approval(env: &Env, signer: &Address) {
        env.storage()
            .temporary()
            .remove(&StorageKey::RelayApproval(signer.clone()));
    }

    // ── Admin transfer (two-step) ─────────────────────────────────────────────

    /// Read the pending admin nominee, if a transfer is in progress.
    pub fn get_pending_admin(env: &Env) -> Option<Address> {
        env.storage().persistent().get(&StorageKey::PendingAdmin)
    }

    /// Persist the pending admin nominee, overwriting any existing proposal.
    pub fn set_pending_admin(env: &Env, nominee: &Address) {
        env.storage()
            .persistent()
            .set(&StorageKey::PendingAdmin, nominee);
    }

    /// Clear the pending admin nominee after a transfer is accepted.
    pub fn clear_pending_admin(env: &Env) {
        env.storage().persistent().remove(&StorageKey::PendingAdmin);
    }

    // ── Schema version ────────────────────────────────────────────────────────

    /// Read the on-chain storage schema version.
    pub fn get_schema_version(env: &Env) -> Result<u32, ContractError> {
        env.storage()
            .persistent()
            .get(&StorageKey::SchemaVersion)
            .ok_or(ContractError::NotInitialised)
    }

    /// Persist the storage schema version. Called once during `initialize()`.
    pub fn set_schema_version(env: &Env, version: u32) {
        env.storage()
            .persistent()
            .set(&StorageKey::SchemaVersion, &version);
    }

    // ── Transactions ──────────────────────────────────────────────────────────

    /// Returns `true` if a transaction record already exists for `tx_id`.
    ///
    /// Existence-only check — unlike [`Self::get_transaction`] it does not
    /// extend TTL, since it is used purely as a pre-write guard against
    /// `transaction_id` reuse (see `register_callback`'s duplicate-tx-id
    /// check, THREAT_MODEL.md finding F-07).
    pub fn transaction_exists(env: &Env, tx_id: &String) -> bool {
        env.storage()
            .persistent()
            .has(&StorageKey::Transaction(tx_id.clone()))
    }

    /// Read a [`Transaction`] by its ID.
    ///
    /// Extends the ledger TTL on each access so active records are never evicted.
    pub fn get_transaction(env: &Env, tx_id: &String) -> Result<Transaction, ContractError> {
        let key = StorageKey::Transaction(tx_id.clone());
        let tx = env
            .storage()
            .persistent()
            .get::<StorageKey, Transaction>(&key)
            .ok_or(ContractError::TransactionNotFound)?;
        env.storage().persistent().extend_ttl(
            &key,
            TRANSACTION_MIN_TTL_LEDGERS,
            TRANSACTION_MIN_TTL_LEDGERS,
        );
        Ok(tx)
    }

    /// Persist (insert or update) a [`Transaction`].
    pub fn save_transaction(env: &Env, tx: &Transaction) {
        let key = StorageKey::Transaction(tx.id.clone());
        env.storage().persistent().set(&key, tx);
        env.storage().persistent().extend_ttl(
            &key,
            TRANSACTION_MIN_TTL_LEDGERS,
            TRANSACTION_MIN_TTL_LEDGERS,
        );
    }

    // ── Merge markers ─────────────────────────────────────────────────────────

    /// Return the canonical tx id `tx_id` was merged into, if any.
    pub fn get_merged_into(env: &Env, tx_id: &String) -> Option<String> {
        env.storage()
            .persistent()
            .get(&StorageKey::MergedInto(tx_id.clone()))
    }

    /// Persist the `MergedInto(canonical)` marker for `duplicate`.
    pub fn set_merged_into(env: &Env, duplicate: &String, canonical: &String) {
        let key = StorageKey::MergedInto(duplicate.clone());
        env.storage().persistent().set(&key, canonical);
        env.storage().persistent().extend_ttl(
            &key,
            TRANSACTION_MIN_TTL_LEDGERS,
            TRANSACTION_MIN_TTL_LEDGERS,
        );
    }

    // ── Forwarding routes ─────────────────────────────────────────────────────

    /// Return the configured `next_phase` for `tx_id`, if any.
    pub fn get_forward_route(env: &Env, tx_id: &String) -> Option<u32> {
        env.storage()
            .persistent()
            .get(&StorageKey::ForwardRoute(tx_id.clone()))
    }

    /// Set (`Some`) or clear (`None`) the forwarding route for `tx_id`.
    pub fn set_forward_route(env: &Env, tx_id: &String, next_phase: Option<u32>) {
        let key = StorageKey::ForwardRoute(tx_id.clone());
        match next_phase {
            Some(p) => env.storage().persistent().set(&key, &p),
            None => env.storage().persistent().remove(&key),
        }
    }

    // ── Idempotency keys ──────────────────────────────────────────────────────

    /// Return the ledger sequence at which an idempotency key was first stored,
    /// or `None` if the key is unknown / expired.
    pub fn get_idempotency_key(env: &Env, key: &String) -> Option<u32> {
        env.storage()
            .temporary()
            .get::<StorageKey, u32>(&StorageKey::IdempotencyKey(key.clone()))
    }

    /// Record an idempotency key with a ~24-hour TTL.
    pub fn set_idempotency_key(env: &Env, key: &String) {
        let storage_key = StorageKey::IdempotencyKey(key.clone());
        env.storage()
            .temporary()
            .set(&storage_key, &env.ledger().sequence());
        env.storage().temporary().extend_ttl(
            &storage_key,
            IDEMPOTENCY_TTL_LEDGERS,
            IDEMPOTENCY_TTL_LEDGERS,
        );
    }
}
