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
//!
//! ## Key namespacing (#89)
//!
//! Every key is [`StorageKey::ns`]`(DataKey)` — see
//! [`crate::types::STORAGE_KEY_NAMESPACE`]. Legacy (schema v1) keys are read
//! only by [`Self::migrate_singleton_keys`].

use soroban_sdk::{Address, BytesN, Env, String, Vec};

use crate::types::{
    ContractError, DataKey, LegacyStorageKey, PendingUpgrade, StorageKey, Transaction,
    UpgradeQuorum, SCHEMA_VERSION,
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
        env.storage()
            .instance()
            .has(&StorageKey::ns(DataKey::Initialised))
    }

    /// Persist the initialised flag.  Called exactly once during `initialize()`.
    pub fn set_initialised(env: &Env) {
        env.storage()
            .instance()
            .set(&StorageKey::ns(DataKey::Initialised), &true);
    }

    // ── Pause / circuit breaker ───────────────────────────────────────────────

    /// Returns `true` when the emergency-pause flag is engaged.
    ///
    /// Defaults to `false` when the flag has never been written, so a freshly
    /// initialised contract is always unpaused.
    pub fn is_paused(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&StorageKey::ns(DataKey::Paused))
            .unwrap_or(false)
    }

    /// Persist the emergency-pause flag.
    pub fn set_paused(env: &Env, paused: bool) {
        env.storage()
            .instance()
            .set(&StorageKey::ns(DataKey::Paused), &paused);
    }

    // ── Admin ─────────────────────────────────────────────────────────────────

    /// Read the current admin address from persistent storage.
    pub fn get_admin(env: &Env) -> Result<Address, ContractError> {
        env.storage()
            .persistent()
            .get(&StorageKey::ns(DataKey::Admin))
            .ok_or(ContractError::NotInitialised)
    }

    /// Persist an admin address.
    pub fn set_admin(env: &Env, admin: &Address) {
        env.storage()
            .persistent()
            .set(&StorageKey::ns(DataKey::Admin), admin);
    }

    // ── Relay signer ──────────────────────────────────────────────────────────

    /// Read the trusted relay signer address.
    pub fn get_relay_signer(env: &Env) -> Result<Address, ContractError> {
        env.storage()
            .persistent()
            .get(&StorageKey::ns(DataKey::RelaySigner))
            .ok_or(ContractError::NotInitialised)
    }

    /// Persist the relay signer address.
    pub fn set_relay_signer(env: &Env, signer: &Address) {
        env.storage()
            .persistent()
            .set(&StorageKey::ns(DataKey::RelaySigner), signer);
    }

    // ── Admin transfer (two-step) ─────────────────────────────────────────────

    /// Read the pending admin nominee, if a transfer is in progress.
    pub fn get_pending_admin(env: &Env) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&StorageKey::ns(DataKey::PendingAdmin))
    }

    /// Persist the pending admin nominee, overwriting any existing proposal.
    pub fn set_pending_admin(env: &Env, nominee: &Address) {
        env.storage()
            .persistent()
            .set(&StorageKey::ns(DataKey::PendingAdmin), nominee);
    }

    /// Clear the pending admin nominee after a transfer is accepted.
    pub fn clear_pending_admin(env: &Env) {
        env.storage()
            .persistent()
            .remove(&StorageKey::ns(DataKey::PendingAdmin));
    }

    // ── Schema version ────────────────────────────────────────────────────────

    /// Read the on-chain storage schema version.
    pub fn get_schema_version(env: &Env) -> Result<u32, ContractError> {
        env.storage()
            .persistent()
            .get(&StorageKey::ns(DataKey::SchemaVersion))
            .ok_or(ContractError::NotInitialised)
    }

    /// Persist the storage schema version. Called once during `initialize()`.
    pub fn set_schema_version(env: &Env, version: u32) {
        env.storage()
            .persistent()
            .set(&StorageKey::ns(DataKey::SchemaVersion), &version);
    }

    // ── Upgrade quorum (#87) ──────────────────────────────────────────────────

    /// Read the optional upgrade quorum configuration.
    pub fn get_upgrade_quorum(env: &Env) -> Option<UpgradeQuorum> {
        env.storage()
            .persistent()
            .get(&StorageKey::ns(DataKey::UpgradeQuorum))
    }

    /// Persist (or clear) the upgrade quorum. `None` removes the key.
    pub fn set_upgrade_quorum(env: &Env, quorum: &Option<UpgradeQuorum>) {
        let key = StorageKey::ns(DataKey::UpgradeQuorum);
        match quorum {
            Some(q) => env.storage().persistent().set(&key, q),
            None => {
                env.storage().persistent().remove(&key);
            }
        }
    }

    /// Read a pending upgrade proposal, if any.
    pub fn get_pending_upgrade(env: &Env) -> Option<PendingUpgrade> {
        env.storage()
            .persistent()
            .get(&StorageKey::ns(DataKey::PendingUpgrade))
    }

    /// Persist a pending upgrade proposal.
    pub fn set_pending_upgrade(env: &Env, pending: &PendingUpgrade) {
        env.storage()
            .persistent()
            .set(&StorageKey::ns(DataKey::PendingUpgrade), pending);
    }

    /// Clear a pending upgrade proposal and its approvals.
    pub fn clear_pending_upgrade(env: &Env) {
        env.storage()
            .persistent()
            .remove(&StorageKey::ns(DataKey::PendingUpgrade));
        env.storage()
            .persistent()
            .remove(&StorageKey::ns(DataKey::UpgradeApprovals));
    }

    /// Read accumulated upgrade co-signer approvals.
    pub fn get_upgrade_approvals(env: &Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&StorageKey::ns(DataKey::UpgradeApprovals))
            .unwrap_or_else(|| Vec::new(env))
    }

    /// Persist accumulated upgrade co-signer approvals.
    pub fn set_upgrade_approvals(env: &Env, approvals: &Vec<Address>) {
        env.storage()
            .persistent()
            .set(&StorageKey::ns(DataKey::UpgradeApprovals), approvals);
    }

    // ── WASM hash provenance (#90) ────────────────────────────────────────────

    /// WASM hash the contract most recently upgraded from, if any.
    pub fn get_previous_wasm_hash(env: &Env) -> Option<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&StorageKey::ns(DataKey::PreviousWasmHash))
    }

    /// Persist the previous WASM hash.
    pub fn set_previous_wasm_hash(env: &Env, hash: &BytesN<32>) {
        env.storage()
            .persistent()
            .set(&StorageKey::ns(DataKey::PreviousWasmHash), hash);
    }

    /// Currently-running WASM hash (genesis at init; updated on every upgrade).
    pub fn get_current_wasm_hash(env: &Env) -> Option<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&StorageKey::ns(DataKey::CurrentWasmHash))
    }

    /// Persist the currently-running WASM hash.
    pub fn set_current_wasm_hash(env: &Env, hash: &BytesN<32>) {
        env.storage()
            .persistent()
            .set(&StorageKey::ns(DataKey::CurrentWasmHash), hash);
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
            .has(&StorageKey::ns(DataKey::Transaction(tx_id.clone())))
    }

    /// Read a [`Transaction`] by its ID.
    ///
    /// Extends the ledger TTL on each access so active records are never evicted.
    pub fn get_transaction(env: &Env, tx_id: &String) -> Result<Transaction, ContractError> {
        let key = StorageKey::ns(DataKey::Transaction(tx_id.clone()));
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
        let key = StorageKey::ns(DataKey::Transaction(tx.id.clone()));
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
            .get::<StorageKey, u32>(&StorageKey::ns(DataKey::IdempotencyKey(key.clone())))
    }

    /// Record an idempotency key with a ~24-hour TTL.
    pub fn set_idempotency_key(env: &Env, key: &String) {
        let storage_key = StorageKey::ns(DataKey::IdempotencyKey(key.clone()));
        env.storage()
            .temporary()
            .set(&storage_key, &env.ledger().sequence());
        env.storage().temporary().extend_ttl(
            &storage_key,
            IDEMPOTENCY_TTL_LEDGERS,
            IDEMPOTENCY_TTL_LEDGERS,
        );
    }

    // ── Schema v1 → v2 key migration (#89) ────────────────────────────────────

    /// One-time migration of legacy (schema v1) singleton keys onto the
    /// namespaced [`StorageKey`] layout, then bump on-chain schema to
    /// [`SCHEMA_VERSION`].
    ///
    /// Per-transaction / idempotency keys are *not* bulk-migrated here: those
    /// are open-ended maps and migrating them requires an off-chain index of
    /// IDs (deployment-ops concern, out of scope for this issue). Singletons
    /// are what gate every privileged path, so they are migrated exhaustively.
    ///
    /// Returns the number of singleton keys rewritten.
    pub fn migrate_singleton_keys(env: &Env) -> Result<u32, ContractError> {
        // Prefer reading schema from the new key; fall back to legacy.
        let on_chain_version: Option<u32> = env
            .storage()
            .persistent()
            .get(&StorageKey::ns(DataKey::SchemaVersion))
            .or_else(|| {
                env.storage()
                    .persistent()
                    .get(&LegacyStorageKey::SchemaVersion)
            });

        match on_chain_version {
            Some(v) if v == SCHEMA_VERSION => return Err(ContractError::NothingToMigrate),
            Some(1) | None => {}
            Some(_) => return Err(ContractError::UnexpectedSchemaForMigration),
        }

        let mut moved = 0u32;

        // Instance: Initialised
        if !env
            .storage()
            .instance()
            .has(&StorageKey::ns(DataKey::Initialised))
            && env.storage().instance().has(&LegacyStorageKey::Initialised)
        {
            let v: bool = env
                .storage()
                .instance()
                .get(&LegacyStorageKey::Initialised)
                .unwrap_or(true);
            env.storage()
                .instance()
                .set(&StorageKey::ns(DataKey::Initialised), &v);
            env.storage()
                .instance()
                .remove(&LegacyStorageKey::Initialised);
            moved = moved.saturating_add(1);
        }

        // Instance: Paused
        if !env
            .storage()
            .instance()
            .has(&StorageKey::ns(DataKey::Paused))
            && env.storage().instance().has(&LegacyStorageKey::Paused)
        {
            let v: bool = env
                .storage()
                .instance()
                .get(&LegacyStorageKey::Paused)
                .unwrap_or(false);
            env.storage()
                .instance()
                .set(&StorageKey::ns(DataKey::Paused), &v);
            env.storage().instance().remove(&LegacyStorageKey::Paused);
            moved = moved.saturating_add(1);
        }

        // Persistent singletons
        moved = moved.saturating_add(Self::migrate_persistent_address(
            env,
            &LegacyStorageKey::Admin,
            DataKey::Admin,
        ));
        moved = moved.saturating_add(Self::migrate_persistent_address(
            env,
            &LegacyStorageKey::RelaySigner,
            DataKey::RelaySigner,
        ));
        moved = moved.saturating_add(Self::migrate_persistent_address(
            env,
            &LegacyStorageKey::PendingAdmin,
            DataKey::PendingAdmin,
        ));

        // Schema version: write new, remove legacy
        if env
            .storage()
            .persistent()
            .has(&LegacyStorageKey::SchemaVersion)
        {
            env.storage()
                .persistent()
                .remove(&LegacyStorageKey::SchemaVersion);
            moved = moved.saturating_add(1);
        }
        Self::set_schema_version(env, SCHEMA_VERSION);

        if moved == 0
            && !env
                .storage()
                .persistent()
                .has(&StorageKey::ns(DataKey::Admin))
        {
            return Err(ContractError::NothingToMigrate);
        }

        Ok(moved)
    }

    fn migrate_persistent_address(env: &Env, legacy: &LegacyStorageKey, data: DataKey) -> u32 {
        let new_key = StorageKey::ns(data);
        if env.storage().persistent().has(&new_key) {
            if env.storage().persistent().has(legacy) {
                env.storage().persistent().remove(legacy);
                return 1;
            }
            return 0;
        }
        if let Some(addr) = env
            .storage()
            .persistent()
            .get::<LegacyStorageKey, Address>(legacy)
        {
            env.storage().persistent().set(&new_key, &addr);
            env.storage().persistent().remove(legacy);
            1
        } else {
            0
        }
    }
}
