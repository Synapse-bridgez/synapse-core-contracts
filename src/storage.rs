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

use soroban_sdk::{Address, Env, String, Vec};

use crate::types::{
    AnchorTierConfig, BondRecord, ContractError, DisputeQueueEntry, DisputeRecord, ParamEntry,
    StorageKey, Transaction, UnbondRequest, DEFAULT_GLOBAL_MAX_AMOUNT,
};

/// Param-registry name of the contract-wide amount ceiling (#169).
pub const PARAM_GLOBAL_MAX_AMOUNT: &str = "global_max_amount";

/// TTL extension in ledgers applied to idempotency keys (~24 hours at ~5s/ledger).
///
/// 24 * 3600 / 5 = `17_280` ledgers.  We round up to `18_000` for safety.
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
    pub fn get_relay_signer(env: &Env) -> Result<Address, ContractError> {
        env.storage()
            .persistent()
            .get(&StorageKey::RelaySigner)
            .ok_or(ContractError::NotInitialised)
    }

    /// Persist the relay signer address.
    pub fn set_relay_signer(env: &Env, signer: &Address) {
        env.storage()
            .persistent()
            .set(&StorageKey::RelaySigner, signer);
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
    /// check, `THREAT_MODEL.md` finding F-07).
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

// ─── Wave 2: Param Registry (#146) ────────────────────────────────────────────

impl StorageClient {
    /// Read a [`ParamEntry`] by name, or `None` if not set.
    pub fn get_param(env: &Env, name: &String) -> Option<ParamEntry> {
        env.storage()
            .persistent()
            .get(&StorageKey::Param(name.clone()))
    }

    /// Persist a [`ParamEntry`].
    pub fn set_param(env: &Env, name: &String, entry: &ParamEntry) {
        env.storage()
            .persistent()
            .set(&StorageKey::Param(name.clone()), entry);
    }
}

// ─── Wave 2: Collateral Bonding (#143) ────────────────────────────────────────

/// Minimum TTL for bond records — same order of magnitude as transaction records.
const BOND_MIN_TTL_LEDGERS: u32 = 100_000;

impl StorageClient {
    /// Read the [`BondRecord`] for a signer, or `None` if not bonded.
    pub fn get_bond_record(env: &Env, signer: &Address) -> Option<BondRecord> {
        let key = StorageKey::BondRecord(signer.clone());
        let record = env.storage().persistent().get(&key)?;
        env.storage()
            .persistent()
            .extend_ttl(&key, BOND_MIN_TTL_LEDGERS, BOND_MIN_TTL_LEDGERS);
        Some(record)
    }

    /// Persist a [`BondRecord`].
    pub fn save_bond_record(env: &Env, record: &BondRecord) {
        let key = StorageKey::BondRecord(record.signer.clone());
        env.storage().persistent().set(&key, record);
        env.storage()
            .persistent()
            .extend_ttl(&key, BOND_MIN_TTL_LEDGERS, BOND_MIN_TTL_LEDGERS);
    }

    /// Remove the [`BondRecord`] for a signer (used when bond reaches zero).
    pub fn remove_bond_record(env: &Env, signer: &Address) {
        env.storage()
            .persistent()
            .remove(&StorageKey::BondRecord(signer.clone()));
    }

    /// Read the pending [`UnbondRequest`] for a signer, or `None`.
    pub fn get_unbond_request(env: &Env, signer: &Address) -> Option<UnbondRequest> {
        env.storage()
            .persistent()
            .get(&StorageKey::UnbondRequest(signer.clone()))
    }

    /// Persist a pending [`UnbondRequest`].
    pub fn save_unbond_request(env: &Env, signer: &Address, request: &UnbondRequest) {
        let key = StorageKey::UnbondRequest(signer.clone());
        env.storage().persistent().set(&key, request);
        env.storage()
            .persistent()
            .extend_ttl(&key, BOND_MIN_TTL_LEDGERS, BOND_MIN_TTL_LEDGERS);
    }

    /// Remove the pending [`UnbondRequest`] for a signer (after claim or slash).
    pub fn remove_unbond_request(env: &Env, signer: &Address) {
        env.storage()
            .persistent()
            .remove(&StorageKey::UnbondRequest(signer.clone()));
    }
}

// ─── Wave 2: Anchor Rebate (#145) ─────────────────────────────────────────────

impl StorageClient {
    /// Read the [`AnchorTierConfig`] for an anchor, or `None` if not set.
    pub fn get_anchor_tier(env: &Env, anchor: &Address) -> Option<AnchorTierConfig> {
        env.storage()
            .persistent()
            .get(&StorageKey::AnchorTier(anchor.clone()))
    }

    /// Persist an [`AnchorTierConfig`].
    pub fn set_anchor_tier(env: &Env, config: &AnchorTierConfig) {
        env.storage()
            .persistent()
            .set(&StorageKey::AnchorTier(config.anchor.clone()), config);
    }
}

// ─── Amount ceilings (#169) ───────────────────────────────────────────────────

impl StorageClient {
    /// Contract-wide ceiling: the `global_max_amount` param if set, else
    /// [`DEFAULT_GLOBAL_MAX_AMOUNT`].
    pub fn get_global_max_amount(env: &Env) -> i128 {
        Self::get_param(env, &String::from_str(env, PARAM_GLOBAL_MAX_AMOUNT))
            .map_or(DEFAULT_GLOBAL_MAX_AMOUNT, |e| e.value)
    }

    /// Explicit per-anchor ceiling for `anchor`, if one was set.
    pub fn get_anchor_ceiling(env: &Env, anchor: &String) -> Option<i128> {
        env.storage()
            .persistent()
            .get(&StorageKey::AnchorCeiling(anchor.clone()))
    }

    /// Set an explicit ceiling for `anchor`.
    pub fn set_anchor_ceiling(env: &Env, anchor: &String, ceiling: i128) {
        env.storage()
            .persistent()
            .set(&StorageKey::AnchorCeiling(anchor.clone()), &ceiling);
    }

    /// Effective ceiling for `anchor`: the stricter (lower) of its explicit
    /// ceiling, if any, and the global ceiling.
    pub fn get_amount_ceiling(env: &Env, anchor: &String) -> i128 {
        let global = Self::get_global_max_amount(env);
        Self::get_anchor_ceiling(env, anchor).map_or(global, |c| c.min(global))
    }
}

// ─── Disputes (#166) ──────────────────────────────────────────────────────────

impl StorageClient {
    /// Open-dispute record for `tx_id`, or `None` if not under dispute.
    pub fn get_dispute(env: &Env, tx_id: &String) -> Option<DisputeRecord> {
        env.storage()
            .persistent()
            .get(&StorageKey::Dispute(tx_id.clone()))
    }

    /// Open-dispute queue, oldest first.
    pub fn get_dispute_queue(env: &Env) -> Vec<DisputeQueueEntry> {
        env.storage()
            .persistent()
            .get(&StorageKey::DisputeQueue)
            .unwrap_or_else(|| Vec::new(env))
    }

    /// Allocate the next dispute sequence number.
    pub fn next_dispute_seq(env: &Env) -> u64 {
        let seq: u64 = env
            .storage()
            .persistent()
            .get(&StorageKey::DisputeSeq)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&StorageKey::DisputeSeq, &(seq + 1));
        seq
    }

    /// Persist `record` and append `tx_id` to the tail of the queue.
    pub fn open_dispute(env: &Env, tx_id: &String, record: &DisputeRecord) {
        env.storage()
            .persistent()
            .set(&StorageKey::Dispute(tx_id.clone()), record);
        let mut queue = Self::get_dispute_queue(env);
        queue.push_back(DisputeQueueEntry {
            seq: record.seq,
            tx_id: tx_id.clone(),
        });
        env.storage()
            .persistent()
            .set(&StorageKey::DisputeQueue, &queue);
    }

    /// Remove the dispute record for `tx_id` and its queue entry.
    pub fn close_dispute(env: &Env, tx_id: &String, seq: u64) {
        env.storage()
            .persistent()
            .remove(&StorageKey::Dispute(tx_id.clone()));
        let mut queue = Self::get_dispute_queue(env);
        if let Some(i) = (0..queue.len()).find(|&i| queue.get_unchecked(i).seq == seq) {
            queue.remove(i);
        }
        env.storage()
            .persistent()
            .set(&StorageKey::DisputeQueue, &queue);
    }
}
