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
//! admin.rs        ← admin / owner management, relay quorum
//! migration.rs    ← versioned upgrade-time migration registry
//! ```

mod admin;
mod events;
mod migration;
mod storage;
mod types;
mod validation;

#[cfg(test)]
mod bench_events;
#[cfg(test)]
mod schema_ci;
#[cfg(test)]
mod test_events_conformance;
#[cfg(test)]
mod test_pause;
#[cfg(test)]
mod test_recovery;
#[cfg(test)]
mod test_upgrade_safety;
#[cfg(test)]
mod test_wave2;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_auth_adversarial;
#[cfg(test)]
mod tests_invariants;
#[cfg(test)]
mod tests_state_machine;

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

use crate::admin::AdminClient;
use crate::events::EventEmitter;
use crate::migration::MigrationRegistry;
use crate::storage::StorageClient;
use crate::types::{
    AnchorTierConfig, BondRecord, CallbackPayload, ContractError, ParamEntry, PendingRelaySigner,
    PendingUpgrade, RelaySignerSet, SchemaCompatRange, SlashEvidence, Transaction,
    TransactionStatus, UnbondRequest, UpgradeCompatibility, UpgradeRecord, UpgradeSnapshot,
    DEFAULT_RELAY_SIGNER_DELAY_LEDGERS, MAX_BATCH_SIZE, MAX_PAGE_LIMIT, MAX_RETRIES,
    SCHEMA_VERSION,
};
use crate::validation::Validator;

/// Default unbonding delay in ledgers (~24 h at 5 s/ledger).
/// Used when the `unbond_delay_ledgers` param has not been set by the admin.
const DEFAULT_UNBOND_DELAY_LEDGERS: i128 = 17_280;

/// Default slash percentage in basis points (10_000 = 100 %).
/// Used when the `slash_bps` param has not been set by the admin.
const DEFAULT_SLASH_BPS: i128 = 10_000;

/// Well-known param names read by the contract's own logic.
/// Operators may also store arbitrary application-level params under other names.
const PARAM_UNBOND_DELAY: &str = "unbond_delay_ledgers";
const PARAM_SLASH_BPS: &str = "slash_bps";
#[allow(dead_code)]
const PARAM_BASE_FEE_BPS: &str = "base_fee_bps";

/// Maximum length (bytes) for a param name string.
const MAX_PARAM_NAME_LEN: u32 = 32;
/// Maximum length (bytes) for an anchor tier label string.
const MAX_TIER_LABEL_LEN: u32 = 16;

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

        // Only the trusted relay signer (plus quorum co-signers, if an N-of-M
        // set is configured) may forward Anchor Platform callbacks.
        AdminClient::require_relay(&env)?;

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

        let tx = Self::persist_new_transaction(&env, &payload);
        Ok(tx.id)
    }

    /// Register up to [`MAX_BATCH_SIZE`] callbacks atomically.
    ///
    /// Relay signer only (`caller` must be a relay signer; the admin cannot
    /// ingest). Every payload is validated, and checked against on-chain and
    /// in-batch duplicate `transaction_id`s, *before* any storage write; any
    /// failure aborts the whole call with no partial writes. An empty or
    /// oversized batch is rejected with [`ContractError::InvalidBatchSize`].
    ///
    /// # Events
    /// Emits [`events::EventTransactionRegistered`] per payload, in batch
    /// order, followed by exactly one [`events::EventBatchProcessed`].
    pub fn batch_register_callback(
        env: Env,
        payloads: Vec<CallbackPayload>,
        caller: Address,
    ) -> Result<u32, ContractError> {
        if StorageClient::is_paused(&env) {
            return Err(ContractError::ContractPaused);
        }
        let n = payloads.len();
        if n == 0 || n > MAX_BATCH_SIZE {
            return Err(ContractError::InvalidBatchSize);
        }
        AdminClient::require_relay_signer(&env, &caller)?;

        // Pass 1: validate everything; no writes.
        for i in 0..n {
            let p = payloads.get_unchecked(i);
            Validator::validate_payload(&env, &p)?;
            if StorageClient::transaction_exists(&env, &p.transaction_id) {
                return Err(ContractError::DuplicateRequest);
            }
            for j in 0..i {
                if payloads.get_unchecked(j).transaction_id == p.transaction_id {
                    return Err(ContractError::DuplicateRequest);
                }
            }
        }

        // Pass 2: write.
        for p in payloads.iter() {
            Self::persist_new_transaction(&env, &p);
        }
        let first = payloads.get_unchecked(0).transaction_id;
        let last = payloads.get_unchecked(n - 1).transaction_id;
        EventEmitter::batch_processed(&env, &caller, n, &first, &last);

        Ok(n)
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
        Self::transition(&env, &mut tx, TransactionStatus::Processing);

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
        tx.stellar_tx_hash = stellar_tx_hash.clone();
        Self::transition(&env, &mut tx, TransactionStatus::Completed);
        EventEmitter::transaction_completed(&env, &tx_id, &stellar_tx_hash);
        Self::emit_forwarding_intent(&env, &tx_id);

        Ok(())
    }

    /// Mark a `Processing` transaction as `Completed` with a settled amount
    /// strictly between zero and the registered amount.
    ///
    /// Full settlements must use [`Self::complete_transaction`]. The settled
    /// amount is stored in [`Transaction::settled_amount`].
    ///
    /// # Events
    /// Emits [`events::EventStatusChanged`], then
    /// [`events::EventTransactionPartiallyCompleted`].
    pub fn partial_complete_transaction(
        env: Env,
        tx_id: String,
        settled_amount: i128,
        stellar_tx_hash: String,
        caller: Address,
    ) -> Result<(), ContractError> {
        AdminClient::assert_is_relay_or_admin(&env, &caller)?;
        Validator::validate_stellar_tx_hash(&stellar_tx_hash)?;

        let mut tx = StorageClient::get_transaction(&env, &tx_id)?;
        if tx.status != TransactionStatus::Processing {
            return Err(ContractError::InvalidStatusTransition);
        }
        Validator::validate_settled_amount(settled_amount, tx.amount)?;
        tx.stellar_tx_hash = stellar_tx_hash.clone();
        tx.settled_amount = Some(settled_amount);
        Self::transition(&env, &mut tx, TransactionStatus::Completed);
        EventEmitter::transaction_partially_completed(
            &env,
            &tx_id,
            tx.amount,
            settled_amount,
            &stellar_tx_hash,
        );
        Self::emit_forwarding_intent(&env, &tx_id);

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
        tx.failure_reason = reason.clone();
        Self::transition(&env, &mut tx, TransactionStatus::Failed);
        EventEmitter::transaction_failed(&env, &tx_id, &reason);

        Ok(())
    }

    /// Cancel a `Pending` or `Processing` transaction (terminal `Cancelled`).
    ///
    /// Callable only by the admin or a relay signer. `Completed` and
    /// `Failed` records are rejected with [`ContractError::CannotCancel`];
    /// an already-`Cancelled` record with [`ContractError::AlreadyCancelled`].
    ///
    /// # Events
    /// Emits [`events::EventStatusChanged`] and
    /// [`events::EventTransactionCancelled`].
    pub fn cancel_transaction(
        env: Env,
        tx_id: String,
        reason: String,
        caller: Address,
    ) -> Result<(), ContractError> {
        AdminClient::assert_is_relay_or_admin(&env, &caller)?;
        Validator::validate_failure_reason(&reason)?;

        let mut tx = StorageClient::get_transaction(&env, &tx_id)?;
        match tx.status {
            TransactionStatus::Pending | TransactionStatus::Processing => {}
            TransactionStatus::Cancelled => return Err(ContractError::AlreadyCancelled),
            _ => return Err(ContractError::CannotCancel),
        }
        tx.failure_reason = reason.clone();
        Self::transition(&env, &mut tx, TransactionStatus::Cancelled);
        EventEmitter::transaction_cancelled(&env, &tx_id, &reason, &caller);

        Ok(())
    }

    /// Move a `Failed` transaction back to `Pending` for reprocessing.
    ///
    /// Relay or admin only. Each call increments [`Transaction::retry_count`];
    /// once it reaches [`MAX_RETRIES`] further calls fail with
    /// [`ContractError::RetryLimitExceeded`]. The counter lives on the
    /// persistent record, so it survives contract upgrades.
    ///
    /// # Events
    /// Emits [`events::EventStatusChanged`] and
    /// [`events::EventTransactionRetried`].
    pub fn retry_transaction(
        env: Env,
        tx_id: String,
        caller: Address,
    ) -> Result<(), ContractError> {
        AdminClient::assert_is_relay_or_admin(&env, &caller)?;

        let mut tx = StorageClient::get_transaction(&env, &tx_id)?;
        if tx.status != TransactionStatus::Failed {
            return Err(ContractError::InvalidStatusTransition);
        }
        if tx.retry_count >= MAX_RETRIES {
            return Err(ContractError::RetryLimitExceeded);
        }
        tx.retry_count += 1;
        tx.failure_reason = String::from_str(&env, "");
        Self::transition(&env, &mut tx, TransactionStatus::Pending);
        EventEmitter::transaction_retried(&env, &tx_id, tx.retry_count);

        Ok(())
    }

    /// Attach a short tag (e.g. `"vip"`, `"manual_review"`) to a transaction.
    ///
    /// Relay or admin only. Tags are non-empty, at most 32 bytes, and capped
    /// at 8 per transaction. They are stored beside the record, not in it.
    ///
    /// # Events
    /// Emits [`events::EventTransactionTagged`].
    pub fn add_transaction_tag(
        env: Env,
        tx_id: String,
        tag: String,
        caller: Address,
    ) -> Result<(), ContractError> {
        AdminClient::assert_is_relay_or_admin(&env, &caller)?;
        if !StorageClient::transaction_exists(&env, &tx_id) {
            return Err(ContractError::TransactionNotFound);
        }
        let mut tags = StorageClient::get_tags(&env, &tx_id);
        Validator::validate_tag(&tag, tags.len())?;
        tags.push_back(tag.clone());
        StorageClient::set_tags(&env, &tx_id, &tags);
        EventEmitter::transaction_tagged(&env, &tx_id, &tag, tags.len());
        Ok(())
    }

    /// Return the tags attached to `tx_id` (empty when untagged).
    pub fn get_transaction_tags(env: Env, tx_id: String) -> Result<Vec<String>, ContractError> {
        if !StorageClient::transaction_exists(&env, &tx_id) {
            return Err(ContractError::TransactionNotFound);
        }
        Ok(StorageClient::get_tags(&env, &tx_id))
    }

    // ── Recovery / phase routing ──────────────────────────────────────────────

    /// **Break-glass admin recovery** (not a routine operation): link
    /// `duplicate_tx_id` to its canonical original after a replay slipped
    /// through as a separate `transaction_id`.
    ///
    /// The duplicate is never deleted: its data stays queryable, it gets a
    /// `MergedInto(canonical_tx_id)` marker (see [`Self::get_merged_into`]),
    /// and its record is moved to `Failed` with `failure_reason = "merged"`
    /// so it cannot pass for an active record. `reason` is the
    /// evidence-backed justification and is carried in the event.
    ///
    /// # Errors
    /// - [`ContractError::MergeSelf`] if both ids are equal.
    /// - [`ContractError::AlreadyMerged`] if either side is already merged.
    /// - [`ContractError::DuplicateSettled`] if the duplicate is `Completed`.
    /// - [`ContractError::TransactionNotFound`] if either record is missing.
    ///
    /// # Events
    /// Emits [`events::EventStatusChanged`] (when the status changes) and
    /// [`events::EventTransactionsMerged`].
    pub fn merge_duplicate_transactions(
        env: Env,
        canonical_tx_id: String,
        duplicate_tx_id: String,
        caller: Address,
        reason: String,
    ) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        if caller != admin {
            return Err(ContractError::Unauthorised);
        }
        if canonical_tx_id == duplicate_tx_id {
            return Err(ContractError::MergeSelf);
        }
        Validator::validate_failure_reason(&reason)?;
        StorageClient::get_transaction(&env, &canonical_tx_id)?;
        let mut dup = StorageClient::get_transaction(&env, &duplicate_tx_id)?;
        if StorageClient::get_merged_into(&env, &duplicate_tx_id).is_some()
            || StorageClient::get_merged_into(&env, &canonical_tx_id).is_some()
        {
            return Err(ContractError::AlreadyMerged);
        }
        if dup.status == TransactionStatus::Completed {
            return Err(ContractError::DuplicateSettled);
        }
        dup.failure_reason = String::from_str(&env, "merged");
        Self::transition(&env, &mut dup, TransactionStatus::Failed);
        StorageClient::set_merged_into(&env, &duplicate_tx_id, &canonical_tx_id);
        EventEmitter::transactions_merged(
            &env,
            &canonical_tx_id,
            &duplicate_tx_id,
            &admin,
            &reason,
        );
        Ok(())
    }

    /// Return the canonical tx id `tx_id` was merged into by
    /// [`Self::merge_duplicate_transactions`], or `None` if not merged.
    pub fn get_merged_into(env: Env, tx_id: String) -> Option<String> {
        StorageClient::get_merged_into(&env, &tx_id)
    }

    /// Configure the phase-router forwarding route for `tx_id`. Admin-gated.
    ///
    /// `next_phase == 0` clears the route (the default: no forwarding).
    /// When set, completing the transaction emits
    /// [`events::EventForwardingIntent`] after the `status`/`done` events.
    /// Never performs a cross-contract call.
    pub fn set_forwarding_route(
        env: Env,
        tx_id: String,
        next_phase: u32,
    ) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        let route = if next_phase == 0 {
            None
        } else {
            Some(next_phase)
        };
        StorageClient::set_forward_route(&env, &tx_id, route);
        Ok(())
    }

    /// Return the configured forwarding `next_phase` for `tx_id`, or `None`.
    pub fn get_forwarding_route(env: Env, tx_id: String) -> Option<u32> {
        StorageClient::get_forward_route(&env, &tx_id)
    }

    // ── Read-only queries ─────────────────────────────────────────────────────

    /// Return the [`Transaction`] for the given `tx_id`, or
    /// [`ContractError::TransactionNotFound`].
    pub fn get_transaction(env: Env, tx_id: String) -> Result<Transaction, ContractError> {
        // Read-only: intentionally NOT gated by the pause flag — pausing must
        // never brick reads.
        StorageClient::get_transaction(&env, &tx_id)
    }

    /// Return a page of transaction IDs currently in `status`.
    ///
    /// * `cursor` — zero-based offset; pass `0` for the first page, then the
    ///   previous `cursor + returned.len()`. An empty page means the end.
    /// * `limit`  — page size, `1..=MAX_PAGE_LIMIT` (else
    ///   [`ContractError::InvalidPageLimit`]).
    ///
    /// Order is insertion order within a status until a transaction leaves
    /// it (removal swaps the last entry into the gap), so a page walk that
    /// races with transitions may skip or repeat an entry. Read-only and not
    /// gated by the pause flag.
    pub fn get_transactions_by_status(
        env: Env,
        status: TransactionStatus,
        cursor: u32,
        limit: u32,
    ) -> Result<Vec<String>, ContractError> {
        if limit == 0 || limit > MAX_PAGE_LIMIT {
            return Err(ContractError::InvalidPageLimit);
        }
        Ok(StorageClient::get_ids_by_status(
            &env, status, cursor, limit,
        ))
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

    /// Rotate the trusted (primary) relay signer address immediately.
    ///
    /// Closed with [`ContractError::TimelockRequired`] once a non-zero
    /// relay-signer delay is configured via [`Self::set_relay_signer_delay`];
    /// rotation must then go through [`Self::propose_relay_signer`].
    ///
    /// # Events
    /// Emits [`events::EventRelaySignerRotated`] so off-chain monitoring can
    /// observe the rotation the same way it does [`Self::accept_admin`].
    pub fn set_relay_signer(env: Env, new_signer: Address) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        if StorageClient::get_relay_signer_delay(&env).unwrap_or(0) > 0 {
            return Err(ContractError::TimelockRequired);
        }
        Self::rotate_primary_relay(&env, &new_signer)
    }

    // ── Timelocked relay-signer rotation ──────────────────────────────────────

    /// Configure the timelock delay (ledgers) for relay-signer rotation.
    /// Admin-gated. A non-zero value also closes the immediate
    /// [`Self::set_relay_signer`] path.
    pub fn set_relay_signer_delay(env: Env, delay_ledgers: u32) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        StorageClient::set_relay_signer_delay(&env, delay_ledgers);
        Ok(())
    }

    /// Start a timelocked rotation of the primary relay signer to
    /// `new_signer`. Admin-gated. A second proposal while one is pending
    /// **replaces** it (restarting the delay).
    ///
    /// # Events
    /// Emits [`events::EventRelaySignerChange`] (topic `rs_prop`).
    pub fn propose_relay_signer(env: Env, new_signer: Address) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        let delay = StorageClient::get_relay_signer_delay(&env)
            .unwrap_or(DEFAULT_RELAY_SIGNER_DELAY_LEDGERS);
        let eta_ledger = env.ledger().sequence().saturating_add(delay);
        StorageClient::set_pending_relay_signer(
            &env,
            &PendingRelaySigner {
                new_signer: new_signer.clone(),
                eta_ledger,
            },
        );
        EventEmitter::relay_signer_proposed(&env, &new_signer, eta_ledger);
        Ok(())
    }

    /// Complete a pending rotation once `eta_ledger` has been reached.
    ///
    /// # Errors
    /// [`ContractError::NoPendingChange`], [`ContractError::TimelockNotElapsed`].
    ///
    /// # Events
    /// Emits [`events::EventRelaySignerRotated`].
    pub fn finalize_relay_signer(env: Env) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        let pending =
            StorageClient::get_pending_relay_signer(&env).ok_or(ContractError::NoPendingChange)?;
        if env.ledger().sequence() < pending.eta_ledger {
            return Err(ContractError::TimelockNotElapsed);
        }
        StorageClient::clear_pending_relay_signer(&env);
        Self::rotate_primary_relay(&env, &pending.new_signer)
    }

    /// Abort a pending rotation. Admin-gated.
    ///
    /// # Events
    /// Emits [`events::EventRelaySignerChange`] (topic `rs_canc`).
    pub fn cancel_relay_signer_change(env: Env) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        let pending =
            StorageClient::get_pending_relay_signer(&env).ok_or(ContractError::NoPendingChange)?;
        StorageClient::clear_pending_relay_signer(&env);
        EventEmitter::relay_signer_change_cancelled(&env, &pending.new_signer, pending.eta_ledger);
        Ok(())
    }

    /// Return the pending relay-signer rotation, if any.
    pub fn pending_relay_signer(env: Env) -> Option<PendingRelaySigner> {
        StorageClient::get_pending_relay_signer(&env)
    }

    // ── Relay signer set (N-of-M) ─────────────────────────────────────────────

    /// Return the relay signer set. Until membership is first changed this is
    /// `{ signers: [relay_signer], threshold: 1 }`.
    pub fn relay_signer_set(env: Env) -> Result<RelaySignerSet, ContractError> {
        StorageClient::get_relay_signer_set(&env)
    }

    /// Register `signer`'s approval for the next relay-gated call
    /// (multi-invocation quorum). Valid for
    /// [`admin::RELAY_APPROVAL_WINDOW_LEDGERS`] and consumed by the gated call.
    pub fn approve_relay_call(env: Env, signer: Address) -> Result<(), ContractError> {
        let set = StorageClient::get_relay_signer_set(&env)?;
        if !set.signers.contains(&signer) {
            return Err(ContractError::NotRelaySigner);
        }
        signer.require_auth();
        StorageClient::set_relay_approval(&env, &signer);
        Ok(())
    }

    /// Add a signer to the relay set. Admin-gated.
    ///
    /// # Events
    /// Emits [`events::EventRelaySignerMembership`] (topic `rs_add`).
    pub fn add_relay_signer(env: Env, signer: Address) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        let mut set = StorageClient::get_relay_signer_set(&env)?;
        if set.signers.contains(&signer) {
            return Err(ContractError::DuplicateRequest);
        }
        set.signers.push_back(signer.clone());
        StorageClient::set_relay_signer_set(&env, &set);
        EventEmitter::relay_signer_added(&env, &signer, set.signers.len());
        Ok(())
    }

    /// Remove a signer from the relay set. Admin-gated; rejected with
    /// [`ContractError::InvalidThreshold`] if it would leave fewer signers
    /// than the threshold. Removing the primary promotes the next signer.
    ///
    /// # Events
    /// Emits [`events::EventRelaySignerMembership`] (topic `rs_rm`).
    pub fn remove_relay_signer(env: Env, signer: Address) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        let mut set = StorageClient::get_relay_signer_set(&env)?;
        let idx = set
            .signers
            .first_index_of(&signer)
            .ok_or(ContractError::NotRelaySigner)?;
        if set.signers.len() - 1 < set.threshold {
            return Err(ContractError::InvalidThreshold);
        }
        set.signers.remove(idx);
        StorageClient::set_relay_signer_set(&env, &set);
        if idx == 0 {
            StorageClient::set_relay_signer(&env, &set.signers.get_unchecked(0));
        }
        StorageClient::clear_relay_approval(&env, &signer);
        EventEmitter::relay_signer_removed(&env, &signer, set.signers.len());
        Ok(())
    }

    /// Change the relay quorum threshold. Admin-gated; must satisfy
    /// `1 <= threshold <= signers.len()`.
    ///
    /// # Events
    /// Emits [`events::EventRelayThresholdChanged`].
    pub fn set_relay_threshold(env: Env, threshold: u32) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        let mut set = StorageClient::get_relay_signer_set(&env)?;
        if threshold == 0 || threshold > set.signers.len() {
            return Err(ContractError::InvalidThreshold);
        }
        let old = set.threshold;
        set.threshold = threshold;
        StorageClient::set_relay_signer_set(&env, &set);
        EventEmitter::relay_threshold_changed(&env, old, threshold);
        Ok(())
    }

    // ── Amount ceilings ───────────────────────────────────────────────────────

    /// Set (`Some`) or clear (`None`) the maximum callback `amount` accepted
    /// for `anchor` (matched against the payload's `asset_issuer`).
    /// Admin-gated; ceilings must be strictly positive.
    ///
    /// # Events
    /// Emits [`events::EventAmountCeilingSet`].
    pub fn set_amount_ceiling(
        env: Env,
        anchor: String,
        ceiling: Option<i128>,
    ) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        Self::check_ceiling(ceiling)?;
        StorageClient::set_amount_ceiling(&env, &anchor, ceiling);
        EventEmitter::amount_ceiling_set(&env, Some(anchor), ceiling, &admin);
        Ok(())
    }

    /// Set (`Some`) or clear (`None`) the ceiling applied to anchors without
    /// their own entry. Admin-gated.
    ///
    /// # Events
    /// Emits [`events::EventAmountCeilingSet`] with `anchor = None`.
    pub fn set_default_amount_ceiling(
        env: Env,
        ceiling: Option<i128>,
    ) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        Self::check_ceiling(ceiling)?;
        StorageClient::set_default_amount_ceiling(&env, ceiling);
        EventEmitter::amount_ceiling_set(&env, None, ceiling, &admin);
        Ok(())
    }

    /// Effective ceiling for `anchor` (`i128::MAX` when none applies).
    pub fn get_amount_ceiling(env: Env, anchor: String) -> i128 {
        StorageClient::get_amount_ceiling(&env, &anchor)
    }

    // ── Contract upgrade ───────────────────────────────────────────────────────

    /// Replace the contract WASM in-place.
    ///
    /// Only the current admin may call this.  The new WASM **must** be compatible
    /// with the existing storage schema (`StorageKey` variants, `Transaction`
    /// struct layout).  Persistent storage (admin, relay_signer, transactions)
    /// and instance storage (init flag, pause flag) survive intact; temporary
    /// storage (idempotency keys) is evicted.
    ///
    /// `expected_schema_version` must match the on-chain `SchemaVersion`
    /// (THREAT_MODEL.md finding F-04). This cannot validate that the *new*
    /// WASM is actually compatible — Soroban gives the running code no way to
    /// introspect an uploaded-but-not-yet-installed WASM blob — but it does
    /// guard against invoking `upgrade()` against a contract instance whose
    /// on-chain state isn't what the caller believes it is.
    ///
    /// # Events
    /// Emits [`events::EventContractUpgraded`] on success.
    ///
    /// # Trust
    /// Because this entry point allows the admin to deploy arbitrary WASM, the
    /// admin key **MUST** be held by a multisig or DAO.  See `DECISIONS.md` for
    /// the full rationale and `README.md` for operational requirements.
    ///
    /// After the swap, [`StorageClient::post_upgrade_self_check`] must pass or
    /// the whole invocation reverts with [`ContractError::SelfCheckFailed`].
    /// A successful upgrade is appended to [`Self::get_upgrade_history`].
    pub fn upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
    ) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        Self::check_schema_compat(&env, expected_schema_version)?;
        Self::swap_wasm(&env, &admin, &new_wasm_hash, false)?;
        Ok(())
    }

    /// Read-only dry run of [`Self::upgrade`]'s guards for `caller`.
    ///
    /// Reports the first guard that would fail, in the same order `upgrade`
    /// checks them, without requiring auth or writing storage. It cannot
    /// check that `new_wasm_hash` is uploaded; that is a host-level check.
    pub fn simulate_upgrade(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
    ) -> UpgradeCompatibility {
        let _ = new_wasm_hash;
        let admin = match StorageClient::get_admin(&env) {
            Ok(a) => a,
            Err(_) => return UpgradeCompatibility::NotInitialised,
        };
        if caller != admin {
            return UpgradeCompatibility::CallerNotAdmin;
        }
        match Self::check_schema_compat(&env, expected_schema_version) {
            Ok(_) => UpgradeCompatibility::Compatible,
            Err(ContractError::NotInitialised) => UpgradeCompatibility::NotInitialised,
            Err(_) => UpgradeCompatibility::SchemaVersionMismatch,
        }
    }

    /// Bounded, oldest-first history of successful upgrades.
    pub fn get_upgrade_history(env: Env) -> Vec<UpgradeRecord> {
        StorageClient::get_upgrade_history(&env)
    }

    /// Seed the WASM hash the contract was deployed with, so the first
    /// upgrade records a real `previous_wasm_hash` and can be rolled back.
    /// Admin-gated; only allowed while no hash is recorded.
    pub fn register_installed_wasm(env: Env, wasm_hash: BytesN<32>) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        if StorageClient::get_current_wasm_hash(&env).is_some() {
            return Err(ContractError::AlreadyInitialised);
        }
        StorageClient::set_current_wasm_hash(&env, &wasm_hash);
        Ok(())
    }

    // ── Timelocked upgrade (ADR-0004) ─────────────────────────────────────────

    /// Configure the `propose_upgrade` → `finalize_upgrade` delay in ledgers.
    /// Admin-gated.
    pub fn set_upgrade_delay(env: Env, delay_ledgers: u32) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        StorageClient::set_upgrade_delay(&env, delay_ledgers);
        Ok(())
    }

    /// Current upgrade timelock delay in ledgers.
    pub fn upgrade_delay(env: Env) -> u32 {
        StorageClient::get_upgrade_delay(&env)
    }

    /// Stage an upgrade that [`Self::finalize_upgrade`] may apply once the
    /// delay elapses. Admin-gated. The schema guard runs up front; a second
    /// proposal replaces the first and restarts the delay.
    ///
    /// # Events
    /// Emits [`events::EventUpgradeProposed`].
    pub fn propose_upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
    ) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        Self::check_schema_compat(&env, expected_schema_version)?;
        let eta_ledger = env
            .ledger()
            .sequence()
            .saturating_add(StorageClient::get_upgrade_delay(&env));
        StorageClient::set_pending_upgrade(
            &env,
            &PendingUpgrade {
                wasm_hash: new_wasm_hash.clone(),
                expected_schema_version,
                eta_ledger,
            },
        );
        EventEmitter::upgrade_proposed(
            &env,
            &admin,
            &new_wasm_hash,
            expected_schema_version,
            eta_ledger,
        );
        Ok(())
    }

    /// Apply the pending upgrade once its ETA has been reached. Admin-gated.
    ///
    /// # Events
    /// Emits `chk_pass`, `upgrade`, then [`events::EventUpgradeFinalized`].
    pub fn finalize_upgrade(env: Env) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        let pending =
            StorageClient::get_pending_upgrade(&env).ok_or(ContractError::NoPendingChange)?;
        if env.ledger().sequence() < pending.eta_ledger {
            return Err(ContractError::TimelockNotElapsed);
        }
        Self::check_schema_compat(&env, pending.expected_schema_version)?;
        StorageClient::clear_pending_upgrade(&env);
        let schema_version = Self::swap_wasm(&env, &admin, &pending.wasm_hash, false)?;
        EventEmitter::upgrade_finalized(&env, &admin, &pending.wasm_hash, schema_version);
        Ok(())
    }

    /// Discard the pending upgrade. Admin-gated.
    ///
    /// # Events
    /// Emits [`events::EventUpgradeCancelled`].
    pub fn cancel_upgrade(env: Env) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        let pending =
            StorageClient::get_pending_upgrade(&env).ok_or(ContractError::NoPendingChange)?;
        StorageClient::clear_pending_upgrade(&env);
        EventEmitter::upgrade_cancelled(&env, &admin, &pending.wasm_hash);
        Ok(())
    }

    /// Return the staged timelocked upgrade, if any.
    pub fn get_pending_upgrade(env: Env) -> Option<PendingUpgrade> {
        StorageClient::get_pending_upgrade(&env)
    }

    // ── Migration / rollback (ADR-0005) ───────────────────────────────────────

    /// Run migration `migration_id` from `migration.rs`, then swap the WASM,
    /// atomically. Admin-gated. Any failure reverts both steps.
    ///
    /// # Events
    /// Emits `chk_pass`, `upgrade`, then [`events::EventUpgradeMigrated`].
    pub fn upgrade_and_migrate(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
        migration_id: u32,
    ) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        Self::check_schema_compat(&env, expected_schema_version)?;
        let touches = MigrationRegistry::run(&env, migration_id)?;
        // A migration that touched no storage leaves the upgrade reversible.
        Self::swap_wasm(&env, &admin, &new_wasm_hash, touches > 0)?;
        EventEmitter::upgrade_migrated(&env, &admin, migration_id, touches, &new_wasm_hash);
        Ok(())
    }

    /// Restore the WASM that was installed before the last upgrade.
    /// Admin-gated.
    ///
    /// # Errors
    /// - [`ContractError::NothingToRollback`] — no previous WASM recorded.
    /// - [`ContractError::UpgradeNotReversible`] — the last upgrade migrated
    ///   storage, so the old WASM may not understand it.
    ///
    /// # Events
    /// Emits `chk_pass`, `upgrade`, then [`events::EventUpgradeRolledBack`].
    pub fn rollback_upgrade(env: Env) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;
        let previous =
            StorageClient::get_previous_upgrade(&env).ok_or(ContractError::NothingToRollback)?;
        if StorageClient::last_upgrade_migrated(&env) {
            return Err(ContractError::UpgradeNotReversible);
        }
        Self::check_schema_compat(&env, previous.schema_version)?;
        let schema_version = Self::swap_wasm(&env, &admin, &previous.wasm_hash, false)?;
        EventEmitter::upgrade_rolled_back(&env, &admin, &previous.wasm_hash, schema_version);
        Ok(())
    }

    /// The snapshot [`Self::rollback_upgrade`] would restore, if any.
    pub fn previous_upgrade(env: Env) -> Option<UpgradeSnapshot> {
        StorageClient::get_previous_upgrade(&env)
    }

    /// Whether the last upgrade migrated storage (blocks rollback).
    pub fn last_upgrade_migrated(env: Env) -> bool {
        StorageClient::last_upgrade_migrated(&env)
    }

    // ── Schema compatibility range (ADR-0006) ─────────────────────────────────

    /// Accepted `expected_schema_version` window for the upgrade entry
    /// points. Defaults to an exact match on [`Self::schema_version`].
    pub fn schema_compatibility_range(env: Env) -> Result<SchemaCompatRange, ContractError> {
        StorageClient::get_schema_compat_range(&env)
    }

    /// Declare the accepted `expected_schema_version` window. Admin-gated.
    /// Must satisfy `min <= schema_version() <= max`, else
    /// [`ContractError::InvalidSchemaCompatRange`].
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

    // ── Wave 2: Param Registry (#146) ─────────────────────────────────────────

    /// Set (or update) a named parameter in the on-chain registry.
    ///
    /// Admin-gated. Param names are freeform strings (max 32 bytes); values are
    /// `i128` scaled integers (e.g. basis points, ledger counts, stroops).
    ///
    /// # Well-known param names
    /// | Name                    | Semantics                                     |
    /// |-------------------------|-----------------------------------------------|
    /// | `unbond_delay_ledgers`  | Ledgers before an unbond request is claimable |
    /// | `slash_bps`             | Slash percentage in basis points (0–10_000)   |
    /// | `base_fee_bps`          | Base fee rate in basis points                 |
    ///
    /// # Events
    /// Emits [`events::EventParamSet`].
    pub fn set_param(env: Env, name: String, value: i128) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;

        if name.is_empty() || name.len() > MAX_PARAM_NAME_LEN {
            return Err(ContractError::InvalidParamName);
        }

        let entry = ParamEntry {
            value,
            updated_at_ledger: env.ledger().sequence(),
            updated_by: admin.clone(),
        };
        StorageClient::set_param(&env, &name, &entry);
        EventEmitter::param_set(&env, &name, value, &admin);
        Ok(())
    }

    /// Read a named parameter from the registry.
    ///
    /// Returns the full [`ParamEntry`] (value + audit fields).
    ///
    /// # Errors
    /// - [`ContractError::ParamNotFound`] — param has never been set.
    pub fn get_param(env: Env, name: String) -> Result<ParamEntry, ContractError> {
        StorageClient::get_param(&env, &name).ok_or(ContractError::ParamNotFound)
    }

    // ── Wave 2: Collateral Bonding (#143) ──────────────────────────────────────

    /// Bond (stake) collateral for the calling relay signer.
    ///
    /// The signer authorises this call with their own key. If they already have
    /// a bond record the `amount` is added on top (top-up semantics).
    ///
    /// # Note
    /// This entry point records the intent to bond; actual token custody
    /// (transfer from signer to contract) will be wired in a future wave once
    /// the token interface is determined. For now the amount is tracked purely
    /// in contract storage as an accounting record.
    ///
    /// # Events
    /// Emits [`events::EventBonded`].
    pub fn bond_collateral(env: Env, signer: Address, amount: i128) -> Result<(), ContractError> {
        signer.require_auth();

        if amount <= 0 {
            return Err(ContractError::InvalidBondAmount);
        }

        let ledger = env.ledger().sequence();
        let new_record = match StorageClient::get_bond_record(&env, &signer) {
            Some(existing) => BondRecord {
                signer: signer.clone(),
                amount: existing.amount + amount,
                bonded_at_ledger: existing.bonded_at_ledger,
                updated_at_ledger: ledger,
            },
            None => BondRecord {
                signer: signer.clone(),
                amount,
                bonded_at_ledger: ledger,
                updated_at_ledger: ledger,
            },
        };

        let total = new_record.amount;
        StorageClient::save_bond_record(&env, &new_record);
        EventEmitter::bonded(&env, &signer, amount, total);
        Ok(())
    }

    /// Initiate an unbond request for the calling relay signer.
    ///
    /// The requested `amount` is locked (still tracked as bonded) until the
    /// unbonding delay elapses and `claim_unbond` is called. This prevents
    /// collateral from being withdrawn the moment before a slash event is
    /// submitted.
    ///
    /// Only one pending unbond request per signer is allowed at a time.
    ///
    /// The delay used is read from the `unbond_delay_ledgers` param if set,
    /// otherwise falls back to [`DEFAULT_UNBOND_DELAY_LEDGERS`] (~24 h).
    ///
    /// # Errors
    /// - [`ContractError::SignerNotBonded`] — signer has no bond record.
    /// - [`ContractError::InsufficientBond`] — requested more than bonded.
    /// - [`ContractError::UnbondAlreadyPending`] — a previous request is not yet claimed.
    ///
    /// # Events
    /// Emits [`events::EventUnbondRequested`].
    pub fn unbond_collateral(env: Env, signer: Address, amount: i128) -> Result<(), ContractError> {
        signer.require_auth();

        if amount <= 0 {
            return Err(ContractError::InvalidBondAmount);
        }

        let record =
            StorageClient::get_bond_record(&env, &signer).ok_or(ContractError::SignerNotBonded)?;

        if amount > record.amount {
            return Err(ContractError::InsufficientBond);
        }

        if StorageClient::get_unbond_request(&env, &signer).is_some() {
            return Err(ContractError::UnbondAlreadyPending);
        }

        // Read the delay from the param registry, falling back to the default.
        let delay = StorageClient::get_param(&env, &String::from_str(&env, PARAM_UNBOND_DELAY))
            .map(|e| e.value)
            .unwrap_or(DEFAULT_UNBOND_DELAY_LEDGERS);

        let now = env.ledger().sequence();
        // Saturating cast: delay is always positive and fits a u32 in practice.
        let claimable_at = now.saturating_add(delay as u32);

        let request = UnbondRequest {
            amount,
            requested_at_ledger: now,
            claimable_at_ledger: claimable_at,
        };
        StorageClient::save_unbond_request(&env, &signer, &request);
        EventEmitter::unbond_requested(&env, &signer, amount, claimable_at);
        Ok(())
    }

    /// Claim a matured unbond request.
    ///
    /// May only be called after the `claimable_at_ledger` recorded in the
    /// pending unbond request has been reached. Reduces the on-chain bond
    /// balance by the previously requested amount and removes the request.
    ///
    /// # Errors
    /// - [`ContractError::NoPendingUnbond`] — no pending unbond for this signer.
    /// - [`ContractError::TimelockNotElapsed`] — too early.
    ///
    /// # Events
    /// Emits [`events::EventUnbondClaimed`].
    pub fn claim_unbond(env: Env, signer: Address) -> Result<(), ContractError> {
        signer.require_auth();

        let request = StorageClient::get_unbond_request(&env, &signer)
            .ok_or(ContractError::NoPendingUnbond)?;

        if env.ledger().sequence() < request.claimable_at_ledger {
            return Err(ContractError::TimelockNotElapsed);
        }

        let amount = request.amount;

        // Reduce the bond. If the result is zero, remove the record entirely.
        if let Some(mut record) = StorageClient::get_bond_record(&env, &signer) {
            record.amount -= amount;
            record.updated_at_ledger = env.ledger().sequence();
            if record.amount == 0 {
                StorageClient::remove_bond_record(&env, &signer);
            } else {
                StorageClient::save_bond_record(&env, &record);
            }
        }

        StorageClient::remove_unbond_request(&env, &signer);
        EventEmitter::unbond_claimed(&env, &signer, amount);
        Ok(())
    }

    /// Read the bond record for a signer, or `None` if not bonded.
    pub fn get_bond_record(env: Env, signer: Address) -> Option<BondRecord> {
        StorageClient::get_bond_record(&env, &signer)
    }

    /// Read the pending unbond request for a signer, or `None`.
    pub fn get_unbond_request(env: Env, signer: Address) -> Option<UnbondRequest> {
        StorageClient::get_unbond_request(&env, &signer)
    }

    // ── Wave 2: Slashing (#144) ────────────────────────────────────────────────

    /// Slash a relay signer's bonded collateral on on-chain-provable evidence
    /// of misbehaviour.
    ///
    /// The only accepted evidence type in Wave 2 is a **conflicting-callback**:
    /// two `CallbackPayload`s sharing the same `transaction_id` but differing
    /// in at least one substantive field (anything other than `idempotency_key`).
    ///
    /// The evidence is verified entirely on-chain — no off-chain oracle or
    /// subjective judgement is involved. Specifically:
    ///
    /// 1. `evidence.payload_a.transaction_id == evidence.tx_id`
    /// 2. `evidence.payload_b.transaction_id == evidence.tx_id`
    /// 3. At least one of `stellar_account`, `amount`, `asset_code`,
    ///    `asset_issuer`, `anchor_transaction_id`, `callback_status` differs
    ///    between `payload_a` and `payload_b`.
    ///
    /// The slash percentage is read from the `slash_bps` param (0–10_000),
    /// defaulting to 10_000 (100 %) if not set.
    ///
    /// # Auth
    /// Admin-gated. A future wave may add guardian-quorum multi-sig here.
    ///
    /// # Errors
    /// - [`ContractError::SignerNotBonded`] — signer has no bond to slash.
    /// - [`ContractError::InvalidSlashEvidence`] — evidence `tx_id` does not match both payloads.
    /// - [`ContractError::InvalidSlashEvidence`] — payloads are identical in all substantive fields.
    ///
    /// # Events
    /// Emits [`events::EventSlashed`].
    pub fn slash_signer(
        env: Env,
        signer: Address,
        evidence: SlashEvidence,
        caller: Address,
    ) -> Result<(), ContractError> {
        // Admin-gated for Wave 2. Guardian-quorum can be added here later.
        AdminClient::require_admin(&env)?;
        caller.require_auth();

        // 1. Verify the evidence tx_id matches both payloads.
        if evidence.payload_a.transaction_id != evidence.tx_id
            || evidence.payload_b.transaction_id != evidence.tx_id
        {
            return Err(ContractError::InvalidSlashEvidence);
        }

        // 2. Verify the payloads differ in at least one substantive field
        //    (idempotency_key is excluded — it is expected to differ per call
        //    and is not a meaningful conflicting signal).
        let conflicting = evidence.payload_a.stellar_account != evidence.payload_b.stellar_account
            || evidence.payload_a.amount != evidence.payload_b.amount
            || evidence.payload_a.asset_code != evidence.payload_b.asset_code
            || evidence.payload_a.asset_issuer != evidence.payload_b.asset_issuer
            || evidence.payload_a.anchor_transaction_id != evidence.payload_b.anchor_transaction_id
            || evidence.payload_a.callback_status != evidence.payload_b.callback_status;

        if !conflicting {
            return Err(ContractError::InvalidSlashEvidence);
        }

        // 3. Load the bond record — there must be collateral to slash.
        let mut record =
            StorageClient::get_bond_record(&env, &signer).ok_or(ContractError::SignerNotBonded)?;

        // 4. Compute the slash amount.
        let slash_bps = StorageClient::get_param(&env, &String::from_str(&env, PARAM_SLASH_BPS))
            .map(|e| e.value)
            .unwrap_or(DEFAULT_SLASH_BPS)
            .clamp(0, 10_000);

        let slashed_amount = (record.amount * slash_bps) / 10_000;
        let remaining = record.amount - slashed_amount;

        // 5. Also cancel any pending unbond request — a signer cannot unbond
        //    after being slashed without re-bonding first.
        StorageClient::remove_unbond_request(&env, &signer);

        // 6. Update or remove the bond record.
        if remaining == 0 {
            StorageClient::remove_bond_record(&env, &signer);
        } else {
            record.amount = remaining;
            record.updated_at_ledger = env.ledger().sequence();
            StorageClient::save_bond_record(&env, &record);
        }

        EventEmitter::slashed(
            &env,
            &signer,
            slashed_amount,
            remaining,
            &evidence.tx_id,
            &caller,
        );
        Ok(())
    }

    // ── Wave 2: Anchor Rebate (#145) ──────────────────────────────────────────

    /// Set (or update) the rebate tier for an anchor.
    ///
    /// Admin-gated. `rebate_bps` must be in `0..=10_000`; `label` is a
    /// human-readable tier name (max 16 bytes, e.g. "gold", "silver").
    ///
    /// # Events
    /// Emits [`events::EventAnchorTierSet`].
    pub fn set_anchor_tier(
        env: Env,
        anchor: Address,
        rebate_bps: u32,
        label: String,
    ) -> Result<(), ContractError> {
        let admin = AdminClient::require_admin(&env)?;

        if rebate_bps > 10_000 {
            return Err(ContractError::InvalidRebateBps);
        }
        if label.len() > MAX_TIER_LABEL_LEN {
            return Err(ContractError::InvalidTierLabel);
        }

        let config = AnchorTierConfig {
            anchor: anchor.clone(),
            rebate_bps,
            label: label.clone(),
            updated_at_ledger: env.ledger().sequence(),
        };
        StorageClient::set_anchor_tier(&env, &config);
        EventEmitter::anchor_tier_set(&env, &anchor, rebate_bps, &label, &admin);
        Ok(())
    }

    /// Read the rebate tier config for an anchor.
    ///
    /// # Errors
    /// - [`ContractError::AnchorTierNotFound`] — no tier has been set for this anchor.
    pub fn get_anchor_tier(env: Env, anchor: Address) -> Result<AnchorTierConfig, ContractError> {
        StorageClient::get_anchor_tier(&env, &anchor).ok_or(ContractError::AnchorTierNotFound)
    }

    /// Compute the effective fee for an anchor given a base fee amount.
    ///
    /// Effective fee = `base_fee × (10_000 − rebate_bps) / 10_000`.
    ///
    /// If no tier has been set for `anchor`, the full `base_fee` is returned
    /// (zero rebate). Emits [`events::EventRebateApplied`] for audit trail.
    ///
    /// The `base_fee` parameter is denominated in the same unit as the
    /// `base_fee_bps` param (basis points of the transaction amount). Callers
    /// should read `base_fee_bps` via `get_param` and pass the result here.
    pub fn compute_effective_fee(
        env: Env,
        anchor: Address,
        base_fee: i128,
    ) -> Result<i128, ContractError> {
        if base_fee < 0 {
            return Err(ContractError::InvalidAmount);
        }

        let (rebate_bps, effective_fee) = match StorageClient::get_anchor_tier(&env, &anchor) {
            Some(config) => {
                let rebate = config.rebate_bps;
                let fee = base_fee * (10_000 - rebate as i128) / 10_000;
                (rebate, fee)
            }
            None => (0u32, base_fee),
        };

        EventEmitter::rebate_applied(&env, &anchor, base_fee, effective_fee, rebate_bps);
        Ok(effective_fee)
    }
}

// ─── Internal helpers (not exported) ─────────────────────────────────────────

impl SynapseCoreContract {
    /// Persist a freshly validated payload as a `Pending` transaction, record
    /// its idempotency key and index entry, and emit `reg`.
    fn persist_new_transaction(env: &Env, payload: &CallbackPayload) -> Transaction {
        let ledger = env.ledger().sequence();
        let empty = String::from_str(env, "");
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
            stellar_tx_hash: empty.clone(),
            failure_reason: empty,
            retry_count: 0,
            settled_amount: None,
        };
        StorageClient::save_transaction(env, &tx);
        StorageClient::index_status_change(env, &tx.id, None, TransactionStatus::Pending);
        StorageClient::set_idempotency_key(env, &payload.idempotency_key);
        EventEmitter::transaction_registered(env, &tx);
        tx
    }

    /// Move `tx` to `new_status`: persist it, update the status index, and
    /// emit `status`. Callers validate the transition first.
    fn transition(env: &Env, tx: &mut Transaction, new_status: TransactionStatus) {
        let old_status = tx.status;
        tx.status = new_status;
        tx.updated_at_ledger = env.ledger().sequence();
        StorageClient::save_transaction(env, tx);
        StorageClient::index_status_change(env, &tx.id, Some(old_status), new_status);
        EventEmitter::status_changed(env, &tx.id, old_status, new_status);
    }

    /// Emit the phase-router forwarding intent if a route is configured.
    fn emit_forwarding_intent(env: &Env, tx_id: &String) {
        if let Some(next_phase) = StorageClient::get_forward_route(env, tx_id) {
            EventEmitter::forwarding_intent(env, tx_id, next_phase);
        }
    }

    /// Replace the primary relay signer, keeping the signer set consistent.
    fn rotate_primary_relay(env: &Env, new_signer: &Address) -> Result<(), ContractError> {
        let old_signer = StorageClient::get_relay_signer(env)?;
        if let Some(set) = StorageClient::get_relay_signer_set_opt(env) {
            if set.signers.first_index_of(new_signer).unwrap_or(0) != 0 {
                return Err(ContractError::DuplicateRequest);
            }
        }
        StorageClient::set_relay_signer(env, new_signer);
        EventEmitter::relay_signer_rotated(env, &old_signer, new_signer);
        Ok(())
    }

    /// Reject non-positive amount ceilings.
    fn check_ceiling(ceiling: Option<i128>) -> Result<(), ContractError> {
        match ceiling {
            Some(c) if c <= 0 => Err(ContractError::InvalidAmount),
            _ => Ok(()),
        }
    }

    /// Require `expected` to fall inside the schema compatibility range.
    fn check_schema_compat(env: &Env, expected: u32) -> Result<(), ContractError> {
        let range = StorageClient::get_schema_compat_range(env)?;
        if expected < range.min || expected > range.max {
            return Err(ContractError::SchemaVersionMismatch);
        }
        Ok(())
    }

    /// Shared WASM-swap primitive for every upgrade path: swap, self-check,
    /// then record history and the rollback snapshot. Returns the on-chain
    /// schema version. Any `Err` reverts the whole invocation.
    fn swap_wasm(
        env: &Env,
        admin: &Address,
        new_wasm_hash: &BytesN<32>,
        migrated: bool,
    ) -> Result<u32, ContractError> {
        let schema_version = StorageClient::get_schema_version(env)?;
        let previous = StorageClient::get_current_wasm_hash(env);

        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());

        if let Err(e) = StorageClient::post_upgrade_self_check(env) {
            EventEmitter::upgrade_self_check_failed(env, schema_version);
            return Err(e);
        }
        EventEmitter::upgrade_self_check_passed(env, schema_version);

        StorageClient::append_upgrade_record(
            env,
            &UpgradeRecord {
                previous_wasm_hash: previous
                    .clone()
                    .unwrap_or(BytesN::from_array(env, &[0u8; 32])),
                new_wasm_hash: new_wasm_hash.clone(),
                schema_version,
                ledger: env.ledger().sequence(),
                admin: admin.clone(),
            },
        );
        match previous {
            Some(wasm_hash) => StorageClient::set_previous_upgrade(
                env,
                &UpgradeSnapshot {
                    wasm_hash,
                    schema_version,
                },
            ),
            None => StorageClient::clear_previous_upgrade(env),
        }
        StorageClient::set_current_wasm_hash(env, new_wasm_hash);
        StorageClient::set_last_upgrade_migrated(env, migrated);
        EventEmitter::contract_upgraded(env, admin, new_wasm_hash, schema_version);
        Ok(schema_version)
    }
}
