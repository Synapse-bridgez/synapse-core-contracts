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

use soroban_sdk::{Address, BytesN, Env, String};

use crate::types::{
    ContractError, PendingUpgrade, SchemaCompatRange, StorageKey, Transaction, UpgradeSnapshot,
    DEFAULT_UPGRADE_DELAY_LEDGERS,
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

    // ── Schema compatibility range ───────────────────────────────────────────

    /// Return the admin-configured compatibility window, defaulting to
    /// exact-match `(current, current)` when unset (ADR-0006 backward compat).
    pub fn get_schema_compat_range(env: &Env, current: u32) -> SchemaCompatRange {
        let min = env
            .storage()
            .persistent()
            .get(&StorageKey::MinCompatibleSchema)
            .unwrap_or(current);
        let max = env
            .storage()
            .persistent()
            .get(&StorageKey::MaxCompatibleSchema)
            .unwrap_or(current);
        SchemaCompatRange { min, max }
    }

    /// Persist the compatibility window. Caller must validate inclusion of
    /// the current schema version before calling.
    pub fn set_schema_compat_range(env: &Env, range: &SchemaCompatRange) {
        env.storage()
            .persistent()
            .set(&StorageKey::MinCompatibleSchema, &range.min);
        env.storage()
            .persistent()
            .set(&StorageKey::MaxCompatibleSchema, &range.max);
    }

    // ── Timelocked upgrade ────────────────────────────────────────────────────

    /// Read the pending upgrade, if any.
    pub fn get_pending_upgrade(env: &Env) -> Option<PendingUpgrade> {
        env.storage().persistent().get(&StorageKey::PendingUpgrade)
    }

    /// Persist (or replace) the pending upgrade proposal.
    pub fn set_pending_upgrade(env: &Env, pending: &PendingUpgrade) {
        env.storage()
            .persistent()
            .set(&StorageKey::PendingUpgrade, pending);
    }

    /// Clear a pending upgrade after finalize or cancel.
    pub fn clear_pending_upgrade(env: &Env) {
        env.storage()
            .persistent()
            .remove(&StorageKey::PendingUpgrade);
    }

    /// Upgrade timelock delay in ledgers (default
    /// [`DEFAULT_UPGRADE_DELAY_LEDGERS`]).
    pub fn get_upgrade_delay(env: &Env) -> u32 {
        env.storage()
            .persistent()
            .get(&StorageKey::UpgradeDelay)
            .unwrap_or(DEFAULT_UPGRADE_DELAY_LEDGERS)
    }

    /// Persist the upgrade timelock delay.
    pub fn set_upgrade_delay(env: &Env, delay_ledgers: u32) {
        env.storage()
            .persistent()
            .set(&StorageKey::UpgradeDelay, &delay_ledgers);
    }

    // ── Upgrade history (single-slot previous) ────────────────────────────────

    /// Currently installed WASM hash, if recorded.
    pub fn get_current_wasm_hash(env: &Env) -> Option<BytesN<32>> {
        env.storage().persistent().get(&StorageKey::CurrentWasmHash)
    }

    /// Record the currently installed WASM hash (post-deploy or post-upgrade).
    pub fn set_current_wasm_hash(env: &Env, hash: &BytesN<32>) {
        env.storage()
            .persistent()
            .set(&StorageKey::CurrentWasmHash, hash);
    }

    /// Previous upgrade snapshot for rollback, if any.
    pub fn get_previous_upgrade(env: &Env) -> Option<UpgradeSnapshot> {
        env.storage().persistent().get(&StorageKey::PreviousUpgrade)
    }

    /// Persist the previous-upgrade snapshot.
    pub fn set_previous_upgrade(env: &Env, snap: &UpgradeSnapshot) {
        env.storage()
            .persistent()
            .set(&StorageKey::PreviousUpgrade, snap);
    }

    /// Clear the previous-upgrade snapshot (e.g. after a successful rollback).
    pub fn clear_previous_upgrade(env: &Env) {
        env.storage()
            .persistent()
            .remove(&StorageKey::PreviousUpgrade);
    }

    /// Whether the most recent upgrade applied a non-reversible migration.
    pub fn last_upgrade_migrated(env: &Env) -> bool {
        env.storage()
            .persistent()
            .get(&StorageKey::LastUpgradeMigrated)
            .unwrap_or(false)
    }

    /// Record whether the most recent upgrade applied a migration.
    pub fn set_last_upgrade_migrated(env: &Env, migrated: bool) {
        env.storage()
            .persistent()
            .set(&StorageKey::LastUpgradeMigrated, &migrated);
    }

    // ── Migration scaffolding marker ──────────────────────────────────────────

    /// Read the migration marker, if present.
    #[cfg(test)]
    pub fn get_migration_marker(env: &Env) -> Option<u32> {
        env.storage().persistent().get(&StorageKey::MigrationMarker)
    }

    /// Write the migration marker (scaffolding / tests).
    pub fn set_migration_marker(env: &Env, migration_id: u32) {
        env.storage()
            .persistent()
            .set(&StorageKey::MigrationMarker, &migration_id);
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
