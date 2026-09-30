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

use soroban_sdk::{Address, BytesN, Env, String, Vec};

use crate::types::{
    AnchorTierConfig, BondRecord, ContractError, ParamEntry, PendingRelaySigner, PendingUpgrade,
    RelaySignerSet, SchemaCompatRange, StorageKey, Transaction, TransactionStatus, UnbondRequest,
    UpgradeRecord, UpgradeSnapshot, DEFAULT_UPGRADE_DELAY_LEDGERS, MAX_UPGRADE_HISTORY,
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

    /// Persist the (primary) relay signer address. When an N-of-M signer set
    /// exists, its primary slot (`signers[0]`) is replaced too.
    pub fn set_relay_signer(env: &Env, signer: &Address) {
        env.storage()
            .persistent()
            .set(&StorageKey::RelaySigner, signer);
        if let Some(mut set) = Self::get_relay_signer_set_opt(env) {
            set.signers.set(0, signer.clone());
            Self::set_relay_signer_set(env, &set);
        }
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

// ─── Per-status index (#176) ──────────────────────────────────────────────────

impl StorageClient {
    fn status_count(env: &Env, status: TransactionStatus) -> u32 {
        env.storage()
            .persistent()
            .get(&StorageKey::StatusCount(status))
            .unwrap_or(0)
    }

    fn put_persistent<V: soroban_sdk::IntoVal<Env, soroban_sdk::Val>>(
        env: &Env,
        key: &StorageKey,
        value: &V,
    ) {
        env.storage().persistent().set(key, value);
        env.storage().persistent().extend_ttl(
            key,
            TRANSACTION_MIN_TTL_LEDGERS,
            TRANSACTION_MIN_TTL_LEDGERS,
        );
    }

    /// Move `tx_id` from `old` (if any) to `new` in the per-status index.
    ///
    /// Every status transition must call this exactly once, alongside
    /// [`Self::save_transaction`]. Removal swaps the last slot of the old
    /// bucket into the vacated one, so each call is O(1) regardless of how
    /// many transactions share a status. Within a status, page order is
    /// therefore insertion order until the first removal from that bucket.
    pub fn index_status_change(
        env: &Env,
        tx_id: &String,
        old: Option<TransactionStatus>,
        new: TransactionStatus,
    ) {
        if let Some(old) = old {
            if old == new {
                return;
            }
            let slot_of = StorageKey::StatusSlotOf(tx_id.clone());
            if let Some(slot) = env.storage().persistent().get::<_, u32>(&slot_of) {
                let count = Self::status_count(env, old);
                let last = count.saturating_sub(1);
                if slot != last {
                    let moved: String = env
                        .storage()
                        .persistent()
                        .get(&StorageKey::StatusSlot(old, last))
                        .unwrap_or_else(|| tx_id.clone());
                    Self::put_persistent(env, &StorageKey::StatusSlot(old, slot), &moved);
                    Self::put_persistent(env, &StorageKey::StatusSlotOf(moved), &slot);
                }
                env.storage()
                    .persistent()
                    .remove(&StorageKey::StatusSlot(old, last));
                Self::put_persistent(env, &StorageKey::StatusCount(old), &last);
            }
        }
        let count = Self::status_count(env, new);
        Self::put_persistent(env, &StorageKey::StatusSlot(new, count), tx_id);
        Self::put_persistent(env, &StorageKey::StatusSlotOf(tx_id.clone()), &count);
        Self::put_persistent(env, &StorageKey::StatusCount(new), &(count + 1));
    }

    /// Return up to `limit` transaction IDs in `status`, starting at slot
    /// `start`.
    pub fn get_ids_by_status(
        env: &Env,
        status: TransactionStatus,
        start: u32,
        limit: u32,
    ) -> Vec<String> {
        let end = start
            .saturating_add(limit)
            .min(Self::status_count(env, status));
        let mut out = Vec::new(env);
        let mut i = start;
        while i < end {
            if let Some(id) = env
                .storage()
                .persistent()
                .get::<_, String>(&StorageKey::StatusSlot(status, i))
            {
                out.push_back(id);
            }
            i += 1;
        }
        out
    }
}

// ─── Tags (#177) ──────────────────────────────────────────────────────────────

impl StorageClient {
    /// Tags attached to `tx_id` (empty when untagged).
    pub fn get_tags(env: &Env, tx_id: &String) -> Vec<String> {
        env.storage()
            .persistent()
            .get(&StorageKey::TxTags(tx_id.clone()))
            .unwrap_or(Vec::new(env))
    }

    /// Persist the tag list for `tx_id`.
    pub fn set_tags(env: &Env, tx_id: &String, tags: &Vec<String>) {
        Self::put_persistent(env, &StorageKey::TxTags(tx_id.clone()), tags);
    }
}

// ─── Amount ceilings (#178) ───────────────────────────────────────────────────

impl StorageClient {
    /// Effective ceiling for `anchor` (the payload's `asset_issuer`): its own
    /// entry, else the contract-wide default, else unlimited.
    pub fn get_amount_ceiling(env: &Env, anchor: &String) -> i128 {
        env.storage()
            .persistent()
            .get(&StorageKey::AmountCeiling(anchor.clone()))
            .or_else(|| Self::get_default_amount_ceiling(env))
            .unwrap_or(i128::MAX)
    }

    /// Set (`Some`) or clear (`None`) the per-anchor ceiling.
    pub fn set_amount_ceiling(env: &Env, anchor: &String, ceiling: Option<i128>) {
        let key = StorageKey::AmountCeiling(anchor.clone());
        match ceiling {
            Some(c) => env.storage().persistent().set(&key, &c),
            None => env.storage().persistent().remove(&key),
        }
    }

    /// Contract-wide default ceiling, if configured. Instance tier: it is read
    /// on every ingestion for anchors without their own entry.
    pub fn get_default_amount_ceiling(env: &Env) -> Option<i128> {
        env.storage()
            .instance()
            .get(&StorageKey::DefaultAmountCeiling)
    }

    /// Set (`Some`) or clear (`None`) the contract-wide default ceiling.
    pub fn set_default_amount_ceiling(env: &Env, ceiling: Option<i128>) {
        match ceiling {
            Some(c) => env
                .storage()
                .instance()
                .set(&StorageKey::DefaultAmountCeiling, &c),
            None => env
                .storage()
                .instance()
                .remove(&StorageKey::DefaultAmountCeiling),
        }
    }
}

// ─── Recovery / forwarding (#179) ─────────────────────────────────────────────

impl StorageClient {
    /// Return the canonical tx id `tx_id` was merged into, if any.
    pub fn get_merged_into(env: &Env, tx_id: &String) -> Option<String> {
        env.storage()
            .persistent()
            .get(&StorageKey::MergedInto(tx_id.clone()))
    }

    /// Persist the `MergedInto(canonical)` marker for `duplicate`.
    pub fn set_merged_into(env: &Env, duplicate: &String, canonical: &String) {
        Self::put_persistent(env, &StorageKey::MergedInto(duplicate.clone()), canonical);
    }

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
            Some(p) => Self::put_persistent(env, &key, &p),
            None => env.storage().persistent().remove(&key),
        }
    }
}

// ─── Relay signer set / timelocked rotation (#179) ────────────────────────────

impl StorageClient {
    /// The explicitly stored signer set, if the admin ever changed it.
    pub fn get_relay_signer_set_opt(env: &Env) -> Option<RelaySignerSet> {
        env.storage().persistent().get(&StorageKey::RelaySignerSet)
    }

    /// Read the relay signer set. Until the admin first changes membership or
    /// threshold, it is the single `RelaySigner` with `threshold = 1`.
    pub fn get_relay_signer_set(env: &Env) -> Result<RelaySignerSet, ContractError> {
        if let Some(set) = Self::get_relay_signer_set_opt(env) {
            return Ok(set);
        }
        let primary = Self::get_relay_signer(env)?;
        Ok(RelaySignerSet {
            signers: soroban_sdk::vec![env, primary],
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

    /// Read the pending relay-signer change, if any.
    pub fn get_pending_relay_signer(env: &Env) -> Option<PendingRelaySigner> {
        env.storage()
            .persistent()
            .get(&StorageKey::PendingRelaySigner)
    }

    /// Persist (overwriting any existing) pending relay-signer change.
    pub fn set_pending_relay_signer(env: &Env, p: &PendingRelaySigner) {
        env.storage()
            .persistent()
            .set(&StorageKey::PendingRelaySigner, p);
    }

    /// Clear the pending relay-signer change.
    pub fn clear_pending_relay_signer(env: &Env) {
        env.storage()
            .persistent()
            .remove(&StorageKey::PendingRelaySigner);
    }

    /// Relay-signer timelock delay in ledgers, if configured.
    pub fn get_relay_signer_delay(env: &Env) -> Option<u32> {
        env.storage()
            .persistent()
            .get(&StorageKey::RelaySignerDelay)
    }

    /// Persist the relay-signer timelock delay in ledgers.
    pub fn set_relay_signer_delay(env: &Env, delay: u32) {
        env.storage()
            .persistent()
            .set(&StorageKey::RelaySignerDelay, &delay);
    }
}

// ─── Upgrade safety (#190, #191) ──────────────────────────────────────────────

impl StorageClient {
    /// Verify the trust-root storage every entry point depends on is intact.
    ///
    /// Run at the end of every WASM swap. Checks that the init flag, admin,
    /// relay signer and schema version are all readable. Any failure maps to
    /// [`ContractError::SelfCheckFailed`], which reverts the whole upgrade.
    pub fn post_upgrade_self_check(env: &Env) -> Result<(), ContractError> {
        let ok = Self::is_initialised(env)
            && Self::get_admin(env).is_ok()
            && Self::get_relay_signer(env).is_ok()
            && Self::get_schema_version(env).is_ok();
        if ok {
            Ok(())
        } else {
            Err(ContractError::SelfCheckFailed)
        }
    }

    /// Bounded, oldest-first upgrade history.
    pub fn get_upgrade_history(env: &Env) -> Vec<UpgradeRecord> {
        env.storage()
            .persistent()
            .get(&StorageKey::UpgradeHistory)
            .unwrap_or(Vec::new(env))
    }

    /// Append `record`, evicting the oldest once [`MAX_UPGRADE_HISTORY`] is
    /// reached.
    pub fn append_upgrade_record(env: &Env, record: &UpgradeRecord) {
        let mut history = Self::get_upgrade_history(env);
        while history.len() >= MAX_UPGRADE_HISTORY {
            history.pop_front();
        }
        history.push_back(record.clone());
        Self::put_persistent(env, &StorageKey::UpgradeHistory, &history);
    }

    /// WASM hash believed to be installed, if known.
    pub fn get_current_wasm_hash(env: &Env) -> Option<BytesN<32>> {
        env.storage().persistent().get(&StorageKey::CurrentWasmHash)
    }

    /// Record the installed WASM hash.
    pub fn set_current_wasm_hash(env: &Env, hash: &BytesN<32>) {
        Self::put_persistent(env, &StorageKey::CurrentWasmHash, hash);
    }

    /// Snapshot `rollback_upgrade` would restore, if any.
    pub fn get_previous_upgrade(env: &Env) -> Option<UpgradeSnapshot> {
        env.storage().persistent().get(&StorageKey::PreviousUpgrade)
    }

    /// Record the pre-upgrade snapshot.
    pub fn set_previous_upgrade(env: &Env, snapshot: &UpgradeSnapshot) {
        Self::put_persistent(env, &StorageKey::PreviousUpgrade, snapshot);
    }

    /// Forget the pre-upgrade snapshot (after a rollback consumed it).
    pub fn clear_previous_upgrade(env: &Env) {
        env.storage()
            .persistent()
            .remove(&StorageKey::PreviousUpgrade);
    }

    /// Whether the last upgrade ran a storage migration.
    pub fn last_upgrade_migrated(env: &Env) -> bool {
        env.storage()
            .persistent()
            .get(&StorageKey::LastUpgradeMigrated)
            .unwrap_or(false)
    }

    /// Record whether the last upgrade ran a storage migration.
    pub fn set_last_upgrade_migrated(env: &Env, migrated: bool) {
        Self::put_persistent(env, &StorageKey::LastUpgradeMigrated, &migrated);
    }

    /// Marker written by scaffolding migrations (see `migration.rs`).
    #[cfg(test)]
    pub fn get_migration_marker(env: &Env) -> Option<u32> {
        env.storage().persistent().get(&StorageKey::MigrationMarker)
    }

    /// Write the scaffolding migration marker.
    pub fn set_migration_marker(env: &Env, migration_id: u32) {
        Self::put_persistent(env, &StorageKey::MigrationMarker, &migration_id);
    }

    /// Pending timelocked upgrade, if any.
    pub fn get_pending_upgrade(env: &Env) -> Option<PendingUpgrade> {
        env.storage().persistent().get(&StorageKey::PendingUpgrade)
    }

    /// Stage (or replace) the pending timelocked upgrade.
    pub fn set_pending_upgrade(env: &Env, pending: &PendingUpgrade) {
        Self::put_persistent(env, &StorageKey::PendingUpgrade, pending);
    }

    /// Clear the pending timelocked upgrade.
    pub fn clear_pending_upgrade(env: &Env) {
        env.storage()
            .persistent()
            .remove(&StorageKey::PendingUpgrade);
    }

    /// Upgrade timelock delay in ledgers.
    pub fn get_upgrade_delay(env: &Env) -> u32 {
        env.storage()
            .persistent()
            .get(&StorageKey::UpgradeDelay)
            .unwrap_or(DEFAULT_UPGRADE_DELAY_LEDGERS)
    }

    /// Persist the upgrade timelock delay in ledgers.
    pub fn set_upgrade_delay(env: &Env, delay: u32) {
        Self::put_persistent(env, &StorageKey::UpgradeDelay, &delay);
    }

    /// Accepted `expected_schema_version` window; defaults to an exact match
    /// on the on-chain schema version (ADR-0006).
    pub fn get_schema_compat_range(env: &Env) -> Result<SchemaCompatRange, ContractError> {
        if let Some(range) = env
            .storage()
            .persistent()
            .get::<_, SchemaCompatRange>(&StorageKey::SchemaCompatRange)
        {
            return Ok(range);
        }
        let current = Self::get_schema_version(env)?;
        Ok(SchemaCompatRange {
            min: current,
            max: current,
        })
    }

    /// Persist the schema compatibility window.
    pub fn set_schema_compat_range(env: &Env, range: &SchemaCompatRange) {
        Self::put_persistent(env, &StorageKey::SchemaCompatRange, range);
    }
}
