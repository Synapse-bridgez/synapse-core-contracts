#![no_std]

//! # Synapse Core — On-Chain Contract
//!
//! Phase 1 of the Synapse Bridge ecosystem.
//!
//! This contract mirrors the off-chain `synapse-core` Rust service, providing an
//! **on-chain transaction registry** that:
//!
//! 1. Accepts callback registrations from the Stellar Anchor Platform (via the
//!    off-chain relay), storing each deposit event with status `Pending`.
//! 2. Guards against duplicate delivery with an idempotency key ledger.
//! 3. Drives the transaction through its lifecycle:
//!    `Pending → Processing → Completed | Failed`
//! 4. Emits structured events at every state transition so Phase 2 (Swap Engine)
//!    and Phase 3 (Cross-Chain Bridge) can subscribe and act.
//!
//! ## Module layout
//!
//! ```text
//! lib.rs          ← you are here (contract entry-point)
//! types.rs        ← Transaction, TransactionStatus, CallbackPayload, errors
//! storage.rs      ← all ledger read/write helpers
//! events.rs       ← typed event emission
//! validation.rs   ← input guards (account format, asset code, amount bounds)
//! admin.rs        ← admin / owner management
//! ```

mod admin;
mod events;
mod migration;
mod storage;
mod types;
mod validation;

#[cfg(test)]
mod test_pause;
#[cfg(test)]
mod test_upgrade_safety;
#[cfg(test)]
mod tests;

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String};

use crate::admin::AdminClient;
use crate::events::EventEmitter;
use crate::migration::MigrationRegistry;
use crate::storage::StorageClient;
use crate::types::{
    CallbackPayload, ContractError, PendingUpgrade, SchemaCompatRange, Transaction,
    TransactionStatus, UpgradeSnapshot, SCHEMA_VERSION,
};
use crate::validation::Validator;

// ─── Public contract interface ───────────────────────────────────────────────

#[contract]
pub struct SynapseCoreContract;

#[contractimpl]
impl SynapseCoreContract {
    // ── Initialisation ────────────────────────────────────────────────────────

    /// Initialise the contract; can only be called once.
    ///
    /// * `admin`        — Address that may call privileged methods.
    /// * `relay_signer` — Address of the trusted off-chain relay that forwards
    ///                    Anchor Platform callbacks on-chain.
    pub fn initialize(
        env: Env,
        admin: Address,
        relay_signer: Address,
    ) -> Result<(), ContractError> {
        if StorageClient::is_initialised(&env) {
            return Err(ContractError::AlreadyInitialised);
        }
        StorageClient::set_admin(&env, &admin);
        StorageClient::set_relay_signer(&env, &relay_signer);
        // Start unpaused so a freshly deployed contract accepts callbacks.
        StorageClient::set_paused(&env, false);
        StorageClient::set_schema_version(&env, SCHEMA_VERSION);
        StorageClient::set_initialised(&env);
        EventEmitter::initialised(&env, &admin, &relay_signer);
        Ok(())
    }

    // ── Callback ingestion (Phase 1 core) ─────────────────────────────────────

    /// Register a new anchor callback, persisting a [`Transaction`] with status
    /// [`TransactionStatus::Pending`].
    ///
    /// Called by the trusted `relay_signer` after the off-chain `synapse-core`
    /// service validates and deduplicates the raw Anchor Platform webhook.
    ///
    /// # Idempotency
    /// If `payload.idempotency_key` has been seen before within the retention
    /// window the call returns `Ok(existing_tx_id)` without writing — matching
    /// the Redis idempotency behaviour of the off-chain service.
    ///
    /// The idempotency key alone is not a durable enough guard: it lives in
    /// *temporary* storage with a ~24h TTL, so a late replay with a fresh
    /// `idempotency_key` but the same `transaction_id` would otherwise pass
    /// the check above and reach the write below. To prevent that write from
    /// silently overwriting an existing (possibly `Completed`/`Failed`)
    /// record, `transaction_id` reuse is also rejected independently of
    /// idempotency-key state (THREAT_MODEL.md finding F-07).
    ///
    /// # Events
    /// Emits [`events::TransactionRegistered`] on first write.
    pub fn register_callback(env: Env, payload: CallbackPayload) -> Result<String, ContractError> {
        // Circuit breaker: while the emergency pause is engaged we fail closed
        // and reject all new callback ingestion outright. This check is first so
        // ingestion is blocked regardless of caller. Read-only queries and
        // draining of already-registered work are intentionally left unguarded
        // (see the module docs on `pause`).
        if StorageClient::is_paused(&env) {
            return Err(ContractError::ContractPaused);
        }

        // Only the trusted relay signer may forward Anchor Platform callbacks.
        let relay = StorageClient::get_relay_signer(&env)?;
        relay.require_auth();

        Validator::validate_payload(&env, &payload)?;

        // Idempotency: a replayed key returns the original tx id without a
        // second write, mirroring the off-chain Redis idempotency behaviour.
        if StorageClient::get_idempotency_key(&env, &payload.idempotency_key).is_some() {
            return Ok(payload.transaction_id.clone());
        }

        // Second-line guard (F-07): the idempotency key's TTL is much shorter
        // than a transaction record's, so a late replay past that window must
        // still not be allowed to overwrite an existing record under the same
        // transaction_id.
        if StorageClient::transaction_exists(&env, &payload.transaction_id) {
            return Err(ContractError::DuplicateRequest);
        }

        let ledger = env.ledger().sequence();
        let tx = Transaction {
            id: payload.transaction_id.clone(),
            stellar_account: payload.stellar_account.clone(),
            amount: payload.amount,
            asset_code: payload.asset_code.clone(),
            asset_issuer: payload.asset_issuer.clone(),
            status: TransactionStatus::Pending,
            created_at_ledger: ledger,
            updated_at_ledger: ledger,
            anchor_transaction_id: payload.anchor_transaction_id.clone(),
            callback_type: payload.callback_type.clone(),
            callback_status: payload.callback_status.clone(),
            stellar_tx_hash: String::from_str(&env, ""),
            failure_reason: String::from_str(&env, ""),
        };

        StorageClient::save_transaction(&env, &tx);
        StorageClient::set_idempotency_key(&env, &payload.idempotency_key);
        EventEmitter::transaction_registered(&env, &tx);

        Ok(tx.id)
    }

    // ── Status transitions ────────────────────────────────────────────────────

    /// Mark a `Pending` transaction as `Processing`.
    ///
    /// Called by the relay when the off-chain processor picks up the job.
    /// Enforces the state machine: only `Pending → Processing` is valid here.
    pub fn start_processing(env: Env, tx_id: String, caller: Address) -> Result<(), ContractError> {
        AdminClient::assert_is_relay_or_admin(&env, &caller)?;

        let mut tx = StorageClient::get_transaction(&env, &tx_id)?;
        if tx.status != TransactionStatus::Pending {
            return Err(ContractError::InvalidStatusTransition);
        }
        let old_status = tx.status.clone();
        tx.status = TransactionStatus::Processing;
        tx.updated_at_ledger = env.ledger().sequence();

        StorageClient::save_transaction(&env, &tx);
        EventEmitter::status_changed(&env, &tx_id, old_status, TransactionStatus::Processing);

        Ok(())
    }

    /// Mark a `Processing` transaction as `Completed` after on-chain verification.
    ///
    /// `stellar_tx_hash` — the Stellar transaction hash confirming the deposit
    ///                     was settled on Horizon. Stored for auditability.
    pub fn complete_transaction(
        env: Env,
        tx_id: String,
        stellar_tx_hash: String,
        caller: Address,
    ) -> Result<(), ContractError> {
        AdminClient::assert_is_relay_or_admin(&env, &caller)?;
        Validator::validate_stellar_tx_hash(&stellar_tx_hash)?;

        let mut tx = StorageClient::get_transaction(&env, &tx_id)?;
        if tx.status != TransactionStatus::Processing {
            return Err(ContractError::InvalidStatusTransition);
        }
        let old_status = tx.status.clone();
        tx.status = TransactionStatus::Completed;
        tx.stellar_tx_hash = stellar_tx_hash.clone();
        tx.updated_at_ledger = env.ledger().sequence();

        StorageClient::save_transaction(&env, &tx);
        EventEmitter::status_changed(&env, &tx_id, old_status, TransactionStatus::Completed);
        EventEmitter::transaction_completed(&env, &tx_id, &stellar_tx_hash);

        Ok(())
    }

    /// Mark a `Pending` or `Processing` transaction as `Failed`.
    ///
    /// `reason` — short human-readable failure code (e.g. "horizon_timeout",
    ///            "invalid_account", "circuit_open").
    pub fn fail_transaction(
        env: Env,
        tx_id: String,
        reason: String,
        caller: Address,
    ) -> Result<(), ContractError> {
        AdminClient::assert_is_relay_or_admin(&env, &caller)?;
        Validator::validate_failure_reason(&reason)?;

        let mut tx = StorageClient::get_transaction(&env, &tx_id)?;
        if tx.status != TransactionStatus::Pending && tx.status != TransactionStatus::Processing {
            return Err(ContractError::InvalidStatusTransition);
        }
        let old_status = tx.status.clone();
        tx.status = TransactionStatus::Failed;
        tx.failure_reason = reason.clone();
        tx.updated_at_ledger = env.ledger().sequence();

        StorageClient::save_transaction(&env, &tx);
        EventEmitter::status_changed(&env, &tx_id, old_status, TransactionStatus::Failed);
        EventEmitter::transaction_failed(&env, &tx_id, &reason);

        Ok(())
    }

    // ── Read-only queries ─────────────────────────────────────────────────────

    /// Return the [`Transaction`] for the given `tx_id`, or
    /// [`ContractError::TransactionNotFound`].
    pub fn get_transaction(env: Env, tx_id: String) -> Result<Transaction, ContractError> {
        // Read-only: intentionally NOT gated by the pause flag — pausing must
        // never brick reads.
        StorageClient::get_transaction(&env, &tx_id)
    }

    /// Return the current [`TransactionStatus`] without fetching the full record.
    pub fn get_status(env: Env, tx_id: String) -> Result<TransactionStatus, ContractError> {
        StorageClient::get_transaction(&env, &tx_id).map(|tx| tx.status)
    }

    /// Check whether an idempotency key has already been processed.
    pub fn is_duplicate(env: Env, idempotency_key: String) -> bool {
        StorageClient::get_idempotency_key(&env, &idempotency_key).is_some()
    }

    /// Return the current admin address, or [`ContractError::NotInitialised`].
    ///
    /// Read-only: lets off-chain monitoring and deployment tooling verify the
    /// on-chain admin against the value recorded in `contract-ids.json`
    /// without needing to trust that record alone.
    pub fn admin(env: Env) -> Result<Address, ContractError> {
        StorageClient::get_admin(&env)
    }

    /// Return the current trusted relay signer address, or
    /// [`ContractError::NotInitialised`].
    pub fn relay_signer(env: Env) -> Result<Address, ContractError> {
        StorageClient::get_relay_signer(&env)
    }

    /// Return the current on-chain storage schema version, or
    /// [`ContractError::NotInitialised`]. The value `upgrade()` requires
    /// callers to pass as `expected_schema_version` when no compatibility
    /// range has been configured (exact-match default).
    pub fn schema_version(env: Env) -> Result<u32, ContractError> {
        StorageClient::get_schema_version(&env)
    }

    /// Return the admin-configured schema compatibility range.
    ///
    /// When unset, both bounds equal the current [`Self::schema_version`]
    /// (exact-match behaviour identical to pre-ADR-0006 deployments).
    pub fn schema_compatibility_range(env: Env) -> Result<SchemaCompatRange, ContractError> {
        let current = StorageClient::get_schema_version(&env)?;
        Ok(StorageClient::get_schema_compat_range(&env, current))
    }

    /// Return the pending admin nominee, if an admin transfer is in
    /// progress. `None` once accepted or if none was ever proposed.
    pub fn pending_admin(env: Env) -> Option<Address> {
        StorageClient::get_pending_admin(&env)
    }

    /// Return the pending timelocked upgrade, if any — hash, expected schema
    /// version, and ETA ledger for off-chain / subscriber monitoring.
    pub fn get_pending_upgrade(env: Env) -> Option<PendingUpgrade> {
        StorageClient::get_pending_upgrade(&env)
    }

    /// Return the configured upgrade timelock delay in ledgers.
    pub fn upgrade_delay(env: Env) -> u32 {
        StorageClient::get_upgrade_delay(&env)
    }

    /// Return the previous-upgrade snapshot used by [`Self::rollback_upgrade`],
    /// if one is recorded.
    pub fn previous_upgrade(env: Env) -> Option<UpgradeSnapshot> {
        StorageClient::get_previous_upgrade(&env)
    }

    /// Return whether the most recent upgrade applied a non-reversible
    /// migration (blocks [`Self::rollback_upgrade`]).
    pub fn last_upgrade_migrated(env: Env) -> bool {
        StorageClient::last_upgrade_migrated(&env)
    }

    // ── Admin (two-step transfer) ────────────────────────────────────────────

    /// Nominate `new_admin` as the next admin.  Requires existing admin auth.
    ///
    /// The transfer does not take effect here — it only completes once
    /// `new_admin` itself calls [`Self::accept_admin`], proving it controls
    /// the corresponding key. A single call from the current admin can no
    /// longer finalise a transfer on its own (THREAT_MODEL.md finding F-03),
    /// which also rules out the classic mis-typed-address failure mode: a
    /// wrong address can never accept, so the current admin simply stays in
    /// control and can propose again.
    ///
    /// Rejects nominating the contract's own address (F-02) — see
    /// [`Validator::validate_admin_nominee`] for why that is the only
    /// "invalid address" Soroban lets this check for on-chain.
    ///
    /// # Events
    /// Emits [`events::EventAdminTransferProposed`].
    pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), ContractError> {
        let current_admin = AdminClient::require_admin(&env)?;
        Validator::validate_admin_nominee(&env, &new_admin)?;
        StorageClient::set_pending_admin(&env, &new_admin);
        EventEmitter::admin_transfer_proposed(&env, &current_admin, &new_admin);
        Ok(())
    }

    /// Complete a pending admin transfer nominated via [`Self::propose_admin`].
    ///
    /// `caller` must be the pending nominee; the call requires `caller`'s own
    /// auth, which is what proves key control and finalises the transfer.
    ///
    /// # Errors
    /// - [`ContractError::NoPendingAdminTransfer`] if no transfer is pending.
    /// - [`ContractError::Unauthorised`] if `caller` is not the pending nominee.
    ///
    /// # Events
    /// Emits [`events::EventAdminTransferred`].
    pub fn accept_admin(env: Env, caller: Address) -> Result<(), ContractError> {
        let pending =
            StorageClient::get_pending_admin(&env).ok_or(ContractError::NoPendingAdminTransfer)?;
        if caller != pending {
            return Err(ContractError::Unauthorised);
        }
        caller.require_auth();

        let old_admin = StorageClient::get_admin(&env)?;
        StorageClient::set_admin(&env, &caller);
        StorageClient::clear_pending_admin(&env);
        EventEmitter::admin_transferred(&env, &old_admin, &caller);
        Ok(())
    }

    /// Rotate the trusted relay signer address.
    ///
    /// # Events
    /// Emits [`events::EventRelaySignerRotated`] so off-chain monitoring can
    /// observe the rotation the same way it does [`Self::accept_admin`].
    pub fn set_relay_signer(env: Env, new_signer: Address) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        let old_signer = StorageClient::get_relay_signer(&env)?;
        StorageClient::set_relay_signer(&env, &new_signer);
        EventEmitter::relay_signer_rotated(&env, &old_signer, &new_signer);
        Ok(())
    }

    // ── Contract upgrade ───────────────────────────────────────────────────────

    /// Replace the contract WASM in-place (immediate; no timelock).
    ///
    /// Prefer [`Self::propose_upgrade`] / [`Self::finalize_upgrade`] for
    /// production upgrades so guardians have a review window (THREAT_MODEL.md
    /// R-05 / ADR-0004). This entry point remains for emergency use,
    /// [`Self::rollback_upgrade`], and as the shared WASM-swap primitive.
    ///
    /// Only the current admin may call this.  The new WASM **must** be compatible
    /// with the existing storage schema (`StorageKey` variants, `Transaction`
    /// struct layout).  Persistent storage (admin, relay_signer, transactions)
    /// and instance storage (init flag, pause flag) survive intact; temporary
    /// storage (idempotency keys) is evicted.
    ///
    /// `expected_schema_version` must fall within the on-chain compatibility
    /// range (default: exact match to `schema_version()` — ADR-0003 / ADR-0006).
    ///
    /// # Events
    /// Emits [`events::EventContractUpgraded`] on success.
    pub fn upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
    ) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        Self::perform_upgrade(&env, &admin, new_wasm_hash, expected_schema_version, false)?;
        Ok(())
    }

    /// Schedule a timelocked upgrade. Admin-gated.
    ///
    /// Starts (or **replaces**) the pending-upgrade window. A second
    /// `propose_upgrade` while one is already pending overwrites the prior
    /// proposal and restarts the delay from the current ledger — documented
    /// replace semantics (same pattern as `propose_admin`).
    ///
    /// # Events
    /// Emits [`events::EventUpgradeProposed`] (topic `up_prop`).
    pub fn propose_upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
    ) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        // Validate the schema argument up front so a doomed proposal is
        // rejected before the timelock starts.
        Self::assert_schema_compatible(&env, expected_schema_version)?;

        let delay = StorageClient::get_upgrade_delay(&env);
        let eta = env.ledger().sequence().saturating_add(delay);
        let pending = PendingUpgrade {
            wasm_hash: new_wasm_hash.clone(),
            expected_schema_version,
            eta_ledger: eta,
        };
        StorageClient::set_pending_upgrade(&env, &pending);
        EventEmitter::upgrade_proposed(&env, &admin, &new_wasm_hash, expected_schema_version, eta);
        Ok(())
    }

    /// Complete a pending timelocked upgrade after the delay has elapsed.
    ///
    /// # Errors
    /// - [`ContractError::NoPendingUpgrade`] if nothing is pending.
    /// - [`ContractError::UpgradeTimelockNotElapsed`] if called before ETA.
    ///
    /// # Events
    /// Emits [`events::EventUpgradeFinalized`] (topic `up_fin`) and the
    /// existing [`events::EventContractUpgraded`] (topic `upgrade`).
    pub fn finalize_upgrade(env: Env) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        let pending =
            StorageClient::get_pending_upgrade(&env).ok_or(ContractError::NoPendingUpgrade)?;
        if env.ledger().sequence() < pending.eta_ledger {
            return Err(ContractError::UpgradeTimelockNotElapsed);
        }
        StorageClient::clear_pending_upgrade(&env);
        Self::perform_upgrade(
            &env,
            &admin,
            pending.wasm_hash.clone(),
            pending.expected_schema_version,
            false,
        )?;
        EventEmitter::upgrade_finalized(
            &env,
            &admin,
            &pending.wasm_hash,
            pending.expected_schema_version,
        );
        Ok(())
    }

    /// Abort a pending timelocked upgrade. Admin-gated.
    ///
    /// # Events
    /// Emits [`events::EventUpgradeCancelled`] (topic `up_can`).
    pub fn cancel_upgrade(env: Env) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        let pending =
            StorageClient::get_pending_upgrade(&env).ok_or(ContractError::NoPendingUpgrade)?;
        StorageClient::clear_pending_upgrade(&env);
        EventEmitter::upgrade_cancelled(&env, &admin, &pending.wasm_hash);
        Ok(())
    }

    /// Configure the upgrade timelock delay in ledgers. Admin-gated.
    pub fn set_upgrade_delay(env: Env, delay_ledgers: u32) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        StorageClient::set_upgrade_delay(&env, delay_ledgers);
        Ok(())
    }

    /// Record the currently installed WASM hash without upgrading.
    ///
    /// Call once after deploy so the first real upgrade can populate the
    /// previous-hash rollback slot (ADR-0004 / issue #83 single-slot history).
    pub fn register_installed_wasm(env: Env, wasm_hash: BytesN<32>) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        StorageClient::set_current_wasm_hash(&env, &wasm_hash);
        Ok(())
    }

    /// Upgrade WASM and run a bounded migration in the same invocation.
    ///
    /// Order: schema-range check → migration registry dispatch → WASM swap.
    /// Any migration `Err` aborts the invoke (Soroban rolls storage back) so
    /// the ledger is unchanged and the WASM is never swapped.
    ///
    /// Sets the non-reversible-migration flag so [`Self::rollback_upgrade`]
    /// will refuse until a later non-migrating upgrade clears it.
    ///
    /// # Events
    /// Emits [`events::EventUpgradeMigrated`] then [`events::EventContractUpgraded`].
    pub fn upgrade_and_migrate(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
        migration_id: u32,
    ) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        Self::assert_schema_compatible(&env, expected_schema_version)?;

        let touches = MigrationRegistry::run(&env, migration_id)?;
        Self::perform_upgrade(
            &env,
            &admin,
            new_wasm_hash.clone(),
            expected_schema_version,
            true,
        )?;
        EventEmitter::upgrade_migrated(&env, &admin, migration_id, touches, &new_wasm_hash);
        Ok(())
    }

    /// Roll back to the immediately-previous WASM hash recorded by the last
    /// successful upgrade. Admin-gated. Single-step only.
    ///
    /// # Errors
    /// - [`ContractError::NothingToRollback`] if no previous snapshot exists.
    /// - [`ContractError::UpgradeNotReversible`] if the latest upgrade ran
    ///   [`Self::upgrade_and_migrate`].
    ///
    /// # Events
    /// Emits [`events::EventUpgradeRolledBack`] (topic `rollback`) and
    /// [`events::EventContractUpgraded`].
    pub fn rollback_upgrade(env: Env) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        if StorageClient::last_upgrade_migrated(&env) {
            return Err(ContractError::UpgradeNotReversible);
        }
        let prev =
            StorageClient::get_previous_upgrade(&env).ok_or(ContractError::NothingToRollback)?;
        StorageClient::clear_previous_upgrade(&env);
        Self::perform_upgrade(
            &env,
            &admin,
            prev.wasm_hash.clone(),
            prev.schema_version,
            false,
        )?;
        EventEmitter::upgrade_rolled_back(&env, &admin, &prev.wasm_hash, prev.schema_version);
        Ok(())
    }

    /// Set the schema compatibility range used by upgrade guards. Admin-gated.
    ///
    /// The range **must** include the current on-chain schema version or the
    /// call is rejected with [`ContractError::InvalidSchemaCompatRange`] —
    /// never silently create an un-upgradeable contract.
    pub fn set_schema_compatibility_range(
        env: Env,
        min: u32,
        max: u32,
    ) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        let current = StorageClient::get_schema_version(&env)?;
        if min > max || current < min || current > max {
            return Err(ContractError::InvalidSchemaCompatRange);
        }
        StorageClient::set_schema_compat_range(&env, &SchemaCompatRange { min, max });
        Ok(())
    }

    // ── Emergency pause / circuit breaker ──────────────────────────────────────

    /// Engage the emergency circuit breaker.  Admin-gated.
    ///
    /// While paused, [`Self::register_callback`] rejects all new ingestion with
    /// [`ContractError::ContractPaused`]. Status transitions
    /// (`start_processing` / `complete_transaction` / `fail_transaction`) are
    /// **deliberately left running** so already-registered work can drain during
    /// an incident, and all read-only queries stay available. Idempotent: pausing
    /// an already-paused contract is a no-op success.
    pub fn pause(env: Env) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        StorageClient::set_paused(&env, true);
        EventEmitter::pause_toggled(&env, true, &admin);
        Ok(())
    }

    /// Release the emergency circuit breaker, resuming normal callback
    /// ingestion.  Admin-gated. Idempotent.
    pub fn unpause(env: Env) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        StorageClient::set_paused(&env, false);
        EventEmitter::pause_toggled(&env, false, &admin);
        Ok(())
    }

    /// Return whether the emergency pause is currently engaged.
    pub fn is_paused(env: Env) -> bool {
        StorageClient::is_paused(&env)
    }

    /// Liveness probe — returns `true` when the contract is initialised.
    pub fn health(env: Env) -> bool {
        StorageClient::is_initialised(&env)
    }

    /// Return the contract version string (semver).
    pub fn version(env: Env) -> String {
        // NOTE: `&'static str` is not a Soroban-representable return type, so the
        // package version is returned as a host `String`.
        String::from_str(&env, env!("CARGO_PKG_VERSION"))
    }
}

impl SynapseCoreContract {
    /// Check `expected` against the configured compatibility range
    /// (exact match when unset).
    fn assert_schema_compatible(env: &Env, expected: u32) -> Result<u32, ContractError> {
        let current = StorageClient::get_schema_version(env)?;
        let range = StorageClient::get_schema_compat_range(env, current);
        if expected < range.min || expected > range.max {
            return Err(ContractError::SchemaVersionMismatch);
        }
        Ok(current)
    }

    /// Shared WASM-swap primitive used by `upgrade`, `finalize_upgrade`,
    /// `upgrade_and_migrate`, and `rollback_upgrade`.
    fn perform_upgrade(
        env: &Env,
        admin: &Address,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
        migrated: bool,
    ) -> Result<(), ContractError> {
        let schema_version = Self::assert_schema_compatible(env, expected_schema_version)?;

        // Snapshot the currently recorded hash so rollback can restore it.
        if let Some(current_hash) = StorageClient::get_current_wasm_hash(env) {
            StorageClient::set_previous_upgrade(
                env,
                &UpgradeSnapshot {
                    wasm_hash: current_hash,
                    schema_version,
                },
            );
        }

        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        StorageClient::set_current_wasm_hash(env, &new_wasm_hash);
        StorageClient::set_last_upgrade_migrated(env, migrated);
        EventEmitter::contract_upgraded(env, admin, &new_wasm_hash, schema_version);
        Ok(())
    }
}
