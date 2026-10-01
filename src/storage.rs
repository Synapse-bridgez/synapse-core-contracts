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
    AdminTransitionRecord, AnchorTierConfig, BondRecord, ContractError, ParamEntry, StorageKey,
    Transaction, UnbondRequest,
};

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

    // ── Admin transition history (#157) ───────────────────────────────────────

    /// Append a single [`AdminTransitionRecord`] to the append-only admin
    /// transition log.
    ///
    /// The log is stored under [`StorageKey::AdminHistory`] and is never
    /// rewritten or pruned, so it forms a complete on-chain provenance chain
    /// for the contract's most powerful role.  Callers are responsible for
    /// populating `record` with the correct transition mechanism so routine
    /// transfers and break-glass revocations remain distinguishable.
    pub fn append_admin_transition(env: &Env, record: &AdminTransitionRecord) {
        let key = StorageKey::AdminHistory;
        let mut history: Vec<AdminTransitionRecord> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        history.push_back(record.clone());
        env.storage().persistent().set(&key, &history);
    }

    /// Read the full append-only admin transition history.
    ///
    /// Returns an empty vector when no transition has been recorded yet (e.g.
    /// immediately after `initialize()`), so callers never need to special-case
    /// a missing key.
    pub fn get_admin_history(env: &Env) -> Vec<AdminTransitionRecord> {
        env.storage()
            .persistent()
            .get(&StorageKey::AdminHistory)
            .unwrap_or_else(|| Vec::new(env))
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
    ///
    /// Also records the current ledger sequence as the transaction's
    /// "last modified" marker in the incremental-sync index (see
    /// [`Self::get_transactions_since`]), so every state transition — new
    /// registrations and updates alike — is observable by off-chain consumers.
    pub fn save_transaction(env: &Env, tx: &Transaction) {
        let key = StorageKey::Transaction(tx.id.clone());
        env.storage().persistent().set(&key, tx);
        env.storage().persistent().extend_ttl(
            &key,
            TRANSACTION_MIN_TTL_LEDGERS,
            TRANSACTION_MIN_TTL_LEDGERS,
        );
        Self::record_transaction_modified(env, &tx.id);
    }

    // ── Incremental sync index (#158) ─────────────────────────────────────────

    /// Record `tx_id` as modified at the current ledger sequence.
    ///
    /// Maintains a monotonically growing, append-only log of
    /// `(ledger_seq, tx_id)` entries under [`StorageKey::TransactionSyncIndex`].
    /// A transaction modified multiple times appears multiple times in the log;
    /// [`Self::get_transactions_since`] de-duplicates so each transaction is
    /// returned at most once per query.
    pub fn record_transaction_modified(env: &Env, tx_id: &String) {
        let key = StorageKey::TransactionSyncIndex;
        let mut index: Vec<(u32, String)> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| Vec::new(env));
        index.push_back((env.ledger().sequence(), tx_id.clone()));
        env.storage().persistent().set(&key, &index);
    }

    /// Return the IDs of every transaction registered or modified strictly
    /// after `ledger_seq`, in ascending last-modified order, de-duplicated so
    /// each transaction appears exactly once.
    ///
    /// `cursor` is an opaque offset into the de-duplicated result set (pass
    /// `None` for the first page).  `limit` caps the page size.  The returned
    /// `Option<u64>` is the cursor to pass on the next call, or `None` when the
    /// result set has been exhausted.
    ///
    /// This is the on-chain primitive for incremental off-chain sync (e.g.
    /// `synapse-core`'s backend): a consumer stores the highest ledger sequence
    /// it has processed and re-queries with that value on each cycle instead of
    /// replaying the full event history.
    pub fn get_transactions_since(
        env: &Env,
        ledger_seq: u32,
        cursor: Option<u64>,
        limit: u32,
    ) -> (Vec<String>, Option<u64>) {
        let index: Vec<(u32, String)> = env
            .storage()
            .persistent()
            .get(&StorageKey::TransactionSyncIndex)
            .unwrap_or_else(|| Vec::new(env));

        // Collect the de-duplicated set of tx IDs modified after `ledger_seq`,
        // preserving first-seen (ascending ledger) order.
        let mut seen: Vec<String> = Vec::new();
        let mut matched: Vec<String> = Vec::new();
        for entry in index.iter() {
            let (seq, tx_id) = entry;
            if seq <= ledger_seq {
                continue;
            }
            if seen.contains(&tx_id) {
                continue;
            }
            seen.push_back(tx_id.clone());
            matched.push_back(tx_id);
        }

        let start = cursor.unwrap_or(0) as u32;
        let total = matched.len();
        let mut page: Vec<String> = Vec::new();
        let mut i = start;
        while i < total && page.len() < limit {
            page.push_back(matched.get(i).unwrap());
            i += 1;
        }

        let next = if i < total { Some(i as u64) } else { None };
        (page, next)
    }

    // ── Idempotency keys ──────────────────────────────────────────────────────

    /// 

