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
mod storage;
mod types;
mod validation;

#[cfg(test)]
mod schema_ci;
#[cfg(test)]
mod test_pause;
#[cfg(test)]
mod test_wave;
#[cfg(test)]
mod tests;

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

use crate::admin::AdminClient;
use crate::events::EventEmitter;
use crate::storage::StorageClient;
use crate::types::{
    CallbackPayload, ContractError, PendingUpgrade, Transaction, TransactionStatus, UpgradeQuorum,
    SCHEMA_VERSION,
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
    /// * `wasm_hash`    — SHA-256 of the WASM being deployed (genesis hash).
    ///                    Recorded so the first [`Self::upgrade`] can report it
    ///                    via [`Self::get_previous_wasm_hash`] (#90).
    pub fn initialize(
        env: Env,
        admin: Address,
        relay_signer: Address,
        wasm_hash: BytesN<32>,
    ) -> Result<(), ContractError> {
        if StorageClient::is_initialised(&env) {
            return Err(ContractError::AlreadyInitialised);
        }
        StorageClient::set_admin(&env, &admin);
        StorageClient::set_relay_signer(&env, &relay_signer);
        // Start unpaused so a freshly deployed contract accepts callbacks.
        StorageClient::set_paused(&env, false);
        StorageClient::set_schema_version(&env, SCHEMA_VERSION);
        StorageClient::set_current_wasm_hash(&env, &wasm_hash);
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
    /// callers to pass as `expected_schema_version`.
    pub fn schema_version(env: Env) -> Result<u32, ContractError> {
        StorageClient::get_schema_version(&env)
    }

    /// Return the pending admin nominee, if an admin transfer is in
    /// progress. `None` once accepted or if none was ever proposed.
    pub fn pending_admin(env: Env) -> Option<Address> {
        StorageClient::get_pending_admin(&env)
    }

    /// Return the WASM hash this contract most recently upgraded from (#90).
    ///
    /// `None` until the first successful [`Self::upgrade`]. After the first
    /// upgrade the value is the genesis WASM hash recorded at
    /// [`Self::initialize`] — never a null/zero default that could be confused
    /// with "never upgraded."
    pub fn get_previous_wasm_hash(env: Env) -> Option<BytesN<32>> {
        StorageClient::get_previous_wasm_hash(&env)
    }

    /// Return the optional upgrade quorum configuration (#87).
    ///
    /// `None` means single-admin upgrade behaviour (backward compatible default).
    pub fn upgrade_quorum(env: Env) -> Option<UpgradeQuorum> {
        StorageClient::get_upgrade_quorum(&env)
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

    /// Configure (or clear) the optional upgrade M-of-N quorum (#87).
    ///
    /// Pass `None` to restore single-admin upgrade behaviour. When `Some`,
    /// [`Self::upgrade`] and [`Self::propose_upgrade`] require genuine
    /// multi-party co-signatures from the member set — admin alone is rejected.
    ///
    /// # Events
    /// Emits [`events::EventUpgradeQuorumSet`].
    pub fn set_upgrade_quorum(
        env: Env,
        quorum: Option<UpgradeQuorum>,
    ) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        if let Some(ref q) = quorum {
            AdminClient::validate_upgrade_quorum(q)?;
        }
        let (threshold, member_count) = match &quorum {
            Some(q) => (q.threshold, q.members.len()),
            None => (0, 0),
        };
        StorageClient::set_upgrade_quorum(&env, &quorum);
        EventEmitter::upgrade_quorum_set(&env, &admin, threshold, member_count);
        Ok(())
    }

    // ── Contract upgrade ───────────────────────────────────────────────────────

    /// Replace the contract WASM in-place.
    ///
    /// Only the current admin may call this.  The new WASM **must** be compatible
    /// with the existing storage schema (`StorageKey` / `DataKey` variants,
    /// `Transaction` struct layout).  Persistent storage and instance storage
    /// survive intact; temporary storage (idempotency keys) is evicted.
    ///
    /// `expected_schema_version` must match the on-chain `SchemaVersion`
    /// (THREAT_MODEL.md finding F-04).
    ///
    /// `cosigners` — when an [`UpgradeQuorum`] is configured (#87), must contain
    /// at least `threshold` distinct quorum members, each of which
    /// `require_auth()`s in this invocation. When no quorum is set, pass an
    /// empty vector (single-admin behaviour).
    ///
    /// On success, records the previously-running WASM hash into
    /// [`Self::get_previous_wasm_hash`] before installing `new_wasm_hash` (#90).
    ///
    /// # Events
    /// Emits [`events::EventContractUpgraded`] on success.
    ///
    /// # Trust
    /// Because this entry point allows the admin to deploy arbitrary WASM, the
    /// admin key **MUST** be held by a multisig or DAO.  See `DECISIONS.md` for
    /// the full rationale and `README.md` for operational requirements. The
    /// optional on-chain upgrade quorum (#87) enforces that expectation at the
    /// contract layer when configured.
    pub fn upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
        cosigners: Vec<Address>,
    ) -> Result<(), ContractError> {
        let admin = AdminClient::require_upgrade_authorisation(&env, &cosigners)?;
        Self::execute_upgrade(&env, &admin, &new_wasm_hash, expected_schema_version)
    }

    /// Stage an upgrade proposal that accumulates quorum co-signatures across
    /// separate transactions (#87).
    ///
    /// Admin-gated. When no upgrade quorum is configured this path is
    /// unnecessary — use [`Self::upgrade`] directly. When a quorum *is* set,
    /// members call [`Self::approve_upgrade`] until threshold is met, at which
    /// point the WASM is installed.
    ///
    /// # Events
    /// Emits [`events::EventUpgradeProposed`].
    pub fn propose_upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
    ) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        // Quorum must be configured for the multi-step path to be meaningful;
        // without one, a single admin call to upgrade() is the right path.
        if StorageClient::get_upgrade_quorum(&env).is_none() {
            return Err(ContractError::InsufficientUpgradeQuorum);
        }
        let schema_version = StorageClient::get_schema_version(&env)?;
        if schema_version != expected_schema_version {
            return Err(ContractError::SchemaVersionMismatch);
        }
        StorageClient::clear_pending_upgrade(&env);
        StorageClient::set_pending_upgrade(
            &env,
            &PendingUpgrade {
                new_wasm_hash: new_wasm_hash.clone(),
                expected_schema_version,
                proposer: admin.clone(),
            },
        );
        EventEmitter::upgrade_proposed(&env, &admin, &new_wasm_hash, expected_schema_version);
        Ok(())
    }

    /// Cast one quorum-member co-signature toward a pending
    /// [`Self::propose_upgrade`] (#87).
    ///
    /// When the accumulated distinct member approvals reach the configured
    /// threshold the upgrade executes immediately (same effects as
    /// [`Self::upgrade`]). Returning `Ok` with the pause still engaged is not
    /// applicable here — incomplete quorum returns
    /// [`ContractError::InsufficientUpgradeQuorum`] is avoided so the vote
    /// commits: this method returns `Ok(())` after recording a vote even when
    /// threshold is not yet met. Callers should check
    /// [`Self::get_previous_wasm_hash`] / events to observe completion.
    ///
    /// # Errors
    /// - [`ContractError::NoPendingUpgrade`]
    /// - [`ContractError::NotUpgradeQuorumMember`]
    pub fn approve_upgrade(env: Env, caller: Address) -> Result<(), ContractError> {
        let pending =
            StorageClient::get_pending_upgrade(&env).ok_or(ContractError::NoPendingUpgrade)?;
        let quorum = StorageClient::get_upgrade_quorum(&env)
            .ok_or(ContractError::InsufficientUpgradeQuorum)?;

        let count = AdminClient::add_upgrade_approval(&env, &caller)?;
        if count < quorum.threshold {
            // Vote recorded; waiting for more co-signatures.
            return Ok(());
        }

        let admin = StorageClient::get_admin(&env)?;
        let result = Self::execute_upgrade(
            &env,
            &admin,
            &pending.new_wasm_hash,
            pending.expected_schema_version,
        );
        StorageClient::clear_pending_upgrade(&env);
        result
    }

    /// One-shot upgrade + singleton key migration helper (#89).
    ///
    /// Installs `new_wasm_hash` (same auth / schema / quorum rules as
    /// [`Self::upgrade`]), then runs [`Self::migrate_storage_keys`]. Intended
    /// for the schema v1 → v2 namespacing cutover. Mainnet execution timing is
    /// a deployment-ops concern; this is the on-chain tooling.
    pub fn upgrade_and_migrate(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
        cosigners: Vec<Address>,
    ) -> Result<u32, ContractError> {
        let admin = AdminClient::require_upgrade_authorisation(&env, &cosigners)?;
        Self::execute_upgrade(&env, &admin, &new_wasm_hash, expected_schema_version)?;
        // Note: migration of *legacy* keys must run in the WASM that still
        // understands both layouts. Operators should call
        // `migrate_storage_keys` on the post-upgrade WASM if this returns
        // NothingToMigrate because the new code no longer sees legacy keys.
        // For same-WASM test / tooling paths we still attempt it here.
        match StorageClient::migrate_singleton_keys(&env) {
            Ok(n) => Ok(n),
            Err(ContractError::NothingToMigrate) => Ok(0),
            Err(e) => Err(e),
        }
    }

    /// Migrate legacy (schema v1) singleton storage keys onto the namespaced
    /// [`crate::types::StorageKey`] layout and bump on-chain schema to
    /// [`SCHEMA_VERSION`] (#89).
    ///
    /// Admin-gated. Idempotent once the namespaced layout is live
    /// ([`ContractError::NothingToMigrate`]).
    pub fn migrate_storage_keys(env: Env) -> Result<u32, ContractError> {
        AdminClient::require_admin_allowing_legacy(&env)?;
        StorageClient::migrate_singleton_keys(&env)
    }

    /// Shared upgrade body: schema check, previous-hash bookkeeping (#90),
    /// WASM replace, event.
    fn execute_upgrade(
        env: &Env,
        admin: &Address,
        new_wasm_hash: &BytesN<32>,
        expected_schema_version: u32,
    ) -> Result<(), ContractError> {
        let schema_version = StorageClient::get_schema_version(env)?;
        if schema_version != expected_schema_version {
            return Err(ContractError::SchemaVersionMismatch);
        }

        let current = StorageClient::get_current_wasm_hash(env)
            .ok_or(ContractError::MissingCurrentWasmHash)?;
        // Always record the hash we are leaving — on the first upgrade this
        // is the genesis hash from initialize(), never a zero default (#90).
        StorageClient::set_previous_wasm_hash(env, &current);

        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());
        StorageClient::set_current_wasm_hash(env, new_wasm_hash);
        EventEmitter::contract_upgraded(env, admin, new_wasm_hash, schema_version);
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
