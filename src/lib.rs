//! Soroban smart contract crate.
//!
//! This crate exposes the contract entry points and supporting modules.
//! The migration registry lives in [`migration`] and is invoked by the
//! upgrade path before the WASM swap commits.
//!
//! Relay signer rotation with timelock lives in [`relay_signer`]:
//! `propose_relay_signer`, `finalize_relay_signer` /
//! `try_finalize_relay_signer`, and `cancel_relay_signer_change`, mirroring
//! the propose/finalize/cancel shape used for admin transfer and upgrades.
//! The immediate single-signer rotation path (`set_relay_signer`) is
//! intentionally separate.

#![no_std]

mod admin;
mod events;
mod migration;
mod relay_signer;
mod storage;
mod types;
mod validation;

#[cfg(test)]
mod bench_events;
#[cfg(test)]
mod bench_resources;
#[cfg(test)]
mod schema_ci;
#[cfg(test)]
mod test_amount_ceiling;
#[cfg(test)]
mod test_batch_budget;
#[cfg(test)]
mod test_dispute_queue;
#[cfg(test)]
mod test_events_conformance;
#[cfg(test)]
mod test_pause;
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

pub use migration::{MigrationRegistry, MigrationRegistryClient};

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env, String, Vec};

use crate::admin::AdminClient;
use crate::events::EventEmitter;
use crate::storage::{StorageClient, PARAM_GLOBAL_MAX_AMOUNT};
use crate::types::{
    AnchorTierConfig, BondRecord, CallbackPayload, ContractError, DisputeRecord, ParamEntry,
    SlashEvidence, Transaction, TransactionStatus, UnbondRequest, MAX_BATCH_SIZE,
    MAX_OPEN_DISPUTES, MAX_PAGE_LIMIT, SCHEMA_VERSION,
};
use crate::validation::Validator;

/// Default unbonding delay in ledgers (~24 h at 5 s/ledger).
/// Used when the `unbond_delay_ledgers` param has not been set by the admin.
const DEFAULT_UNBOND_DELAY_LEDGERS: i128 = 17_280;

/// Default slash percentage in basis points (`10_000` = 100 %).
/// Used when the `slash_bps` param has not been set by the admin.
const DEFAULT_SLASH_BPS: i128 = 10_000;

/// Well-known param names read by the contract's own logic.
/// Operators may also store arbitrary application-level params under other names.
const PARAM_UNBOND_DELAY: &str = "unbond_delay_ledgers";
const PARAM_SLASH_BPS: &str = "slash_bps";
#[allow(dead_code)] // documented well-known name; read off-chain until fee accrual lands (#141)
const PARAM_BASE_FEE_BPS: &str = "base_fee_bps";

/// Maximum length (bytes) for a param name string.
const MAX_PARAM_NAME_LEN: u32 = 32;
/// Maximum length (bytes) for an anchor tier label string.
const MAX_TIER_LABEL_LEN: u32 = 16;

// ─── Public contract interface ───────────────────────────────────────────────

#[contract]
pub struct SynapseCoreContract;

#[contractimpl]
#[allow(clippy::needless_pass_by_value)] // the Soroban contract ABI requires by-value arguments
impl SynapseCoreContract {
    // ── Initialisation ────────────────────────────────────────────────────────

    /// Initialise the contract; can only be called once.
    ///
    /// * `admin`        — Address that may call privileged methods.
    /// * `relay_signer` — Address of the trusted off-chain relay that forwards
    ///                    Anchor Platform callbacks on-chain.
    ///
    /// # Errors
    /// - [`ContractError::AlreadyInitialised`] if called a second time.
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
    /// idempotency-key state (`THREAT_MODEL.md` finding F-07).
    ///
    /// # Events
    /// Emits [`events::TransactionRegistered`] on first write.
    ///
    /// # Errors
    /// - [`ContractError::ContractPaused`] while the emergency pause is engaged.
    /// - [`ContractError::NotInitialised`] before `initialize`.
    /// - Any payload-validation error from [`Validator::validate_payload`], including
    ///   [`ContractError::AmountCeilingExceeded`].
    /// - [`ContractError::DuplicateRequest`] if `transaction_id` already exists.
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
            return Ok(payload.transaction_id);
        }

        // Second-line guard (F-07): the idempotency key's TTL is much shorter
        // than a transaction record's, so a late replay past that window must
        // still not be allowed to overwrite an existing record under the same
        // transaction_id.
        if StorageClient::transaction_exists(&env, &payload.transaction_id) {
            return Err(ContractError::DuplicateRequest);
        }

        Ok(write_pending(&env, &payload))
    }

    /// Register up to [`MAX_BATCH_SIZE`] callbacks atomically.
    ///
    /// Relay signer only. Every check below runs before any storage write,
    /// so any failure aborts the whole call with no partial writes. Unlike
    /// [`Self::register_callback`], a replayed idempotency key is not a
    /// silent success here: every item must be new.
    ///
    /// # Events
    /// Emits [`events::EventTransactionRegistered`] per payload followed by
    /// one [`events::EventBatchProcessed`].
    ///
    /// # Errors
    /// - [`ContractError::ContractPaused`] while the emergency pause is engaged.
    /// - [`ContractError::NotRelaySigner`] if `caller` is not the relay signer.
    /// - [`ContractError::InvalidBatchSize`] for an empty batch or one larger
    ///   than [`MAX_BATCH_SIZE`].
    /// - [`ContractError::BatchBudgetExceeded`] if the batch's conservative
    ///   worst-case write/event byte estimate exceeds the safety budget,
    ///   regardless of item count (see [`Validator::estimate_batch_cost`] and
    ///   `COST_MODEL.md` §11.1).
    /// - Any payload-validation error from [`Validator::validate_payload`].
    /// - [`ContractError::DuplicateRequest`] if a `transaction_id` or
    ///   idempotency key already exists on-chain or repeats within the batch.
    pub fn batch_register_callback(
        env: Env,
        payloads: Vec<CallbackPayload>,
        caller: Address,
    ) -> Result<u32, ContractError> {
        if StorageClient::is_paused(&env) {
            return Err(ContractError::ContractPaused);
        }
        AdminClient::require_relay_signer(&env, &caller)?;

        let n = payloads.len();
        if n == 0 || n > MAX_BATCH_SIZE {
            return Err(ContractError::InvalidBatchSize);
        }
        Validator::validate_batch_budget(&payloads)?;

        // Pass 1: validate everything; no writes.
        for i in 0..n {
            let p = payloads.get_unchecked(i);
            Validator::validate_payload(&env, &p)?;
            if StorageClient::get_idempotency_key(&env, &p.idempotency_key).is_some()
                || StorageClient::transaction_exists(&env, &p.transaction_id)
            {
                return Err(ContractError::DuplicateRequest);
            }
            for j in 0..i {
                let q = payloads.get_unchecked(j);
                if q.transaction_id == p.transaction_id || q.idempotency_key == p.idempotency_key {
                    return Err(ContractError::DuplicateRequest);
                }
            }
        }

        // Pass 2: write.
        for p in payloads.iter() {
            write_pending(&env, &p);
        }
        EventEmitter::batch_processed(
            &env,
            &caller,
            n,
            &payloads.get_unchecked(0).transaction_id,
            &payloads.get_unchecked(n - 1).transaction_id,
        );
        Ok(n)
    }

    // ── Status transitions ────────────────────────────────────────────────────

    /// Mark a `Pending` transaction as `Processing`.
    ///
    /// Called by the relay when the off-chain processor picks up the job.
    /// Enforces the state machine: only `Pending → Processing` is valid here.
    ///
    /// # Errors
    /// - [`ContractError::Unauthorised`] if `caller` is neither admin nor relay.
    /// - [`ContractError::TransactionNotFound`] for an unknown `tx_id`.
    /// - [`ContractError::InvalidStatusTransition`] unless the transaction is `Pending`.
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

    /// Propose a new admin. Only the current admin may call this.
    ///
    /// Overwrites any previously pending proposal rather than stacking.
    ///
    /// # Events
    /// Emits [`events::EventAdminTransferProposed`].
    ///
    /// # Errors
    /// - [`ContractError::NotInitialised`] before `initialize`.
    /// - [`ContractError::InvalidAdminNominee`] if `new_admin` is this contract.
    pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), ContractError> {
        let current_admin = AdminClient::require_admin(&env)?;
        Validator::validate_admin_nominee(&env, &new_admin)?;
        StorageClient::set_pending_admin(&env, &new_admin);
        EventEmitter::admin_transfer_proposed(&env, &current_admin, &new_admin);
        Ok(())
    }

    /// Accept a pending admin transfer. Only the stored `pending_admin` may call this.
    ///
    /// Finalizes the transfer, clears `pending_admin`, and emits the `admin` event.
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

    /// Read-only query returning the current admin, if set.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialised`] before `initialize`.
    pub fn admin(env: Env) -> Result<Address, ContractError> {
        StorageClient::get_admin(&env)
    }

    /// Read-only query returning the pending admin proposal, if any.
    pub fn pending_admin(env: Env) -> Option<Address> {
        StorageClient::get_pending_admin(&env)
    }

    /// Rotate the single relay signer immediately (no timelock).
    ///
    /// Admin-gated via the shared `require_admin` helper. Persists `new_signer`
    /// so `relay_signer()` returns it right away and `register_callback`
    /// authenticates against the updated value. Emits `EventRelaySignerRotated`
    /// carrying the old and new signer addresses on every successful call,
    /// including no-op rotations to the currently-active signer.
    ///
    /// # Events
    /// Emits [`events::EventRelaySignerRotated`] so off-chain monitoring can
    /// observe the rotation the same way it does [`Self::accept_admin`].
    ///
    /// # Errors
    /// - [`ContractError::NotInitialised`] before `initialize`.
    pub fn set_relay_signer(env: Env, new_signer: Address) -> Result<(), ContractError> {
        AdminClient::require_admin(&env)?;
        let old_signer = StorageClient::get_relay_signer(&env)?;
        StorageClient::set_relay_signer(&env, &new_signer);
        EventEmitter::relay_signer_rotated(&env, &old_signer, &new_signer);
        Ok(())
    }

    /// Returns the live schema version of the contract's storage layout.
    ///
    /// This is a cheap query backed by [`SCHEMA_VERSION`]; it is not derived
    /// from WASM metadata.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialised`] before `initialize`.
    pub fn schema_version(env: Env) -> Result<u32, ContractError> {
        StorageClient::get_schema_version(&env)
    }

    /// Return the current contract semver.
    ///
    /// Stable across calls within a single deployment and independent from
    /// `schema_version()`. Bumped on schema changes per `EVENTS.md`.
    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    /// Minimal liveness / self-consistency check.
    ///
    /// Returns `true` only when both the admin and relay signer are set. Safe to
    /// call before `initialize()`: it reports `false` rather than panicking.
    pub fn health(env: Env) -> bool {
        StorageClient::get_admin(&env).is_some() && StorageClient::get_relay_signer(&env).is_some()
    }

    /// Upgrades the contract WASM to `new_wasm_hash`.
    ///
    /// Admin-gated via `require_auth()`. The caller's `expected_schema_version`
    /// must match the live [`schema_version`](Self::schema_version); otherwise
    /// the call is rejected with [`ContractError::SchemaVersionMismatch`]
    /// *before* `env.deployer().update_current_contract_wasm()` is invoked.
    ///
    /// On success, `EventContractUpgraded` ("upgrade" topic) is emitted.
    /// Persistent and instance storage survive the upgrade; only temporary
    /// storage (idempotency keys) may be evicted.
    ///
    /// # Trust
    /// Because this entry point allows the admin to deploy arbitrary WASM, the
    /// admin key **MUST** be held by a multisig or DAO.  See `DECISIONS.md` for
    /// the full rationale and `README.md` for operational requirements.
    ///
    /// # Errors
    /// - [`ContractError::NotInitialised`] before `initialize`.
    /// - [`ContractError::SchemaVersionMismatch`] if `expected_schema_version` differs.
    pub fn upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
    ) -> Result<(), ContractError> {
        // Cheapest-first ordering: authenticate the admin before any other work.
        let admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .ok_or(ContractError::Unauthorized)?;
        admin.require_auth();

        // Schema guard: reject mismatches before touching WASM.
        if expected_schema_version != SCHEMA_VERSION {
            return Err(ContractError::SchemaVersionMismatch);
        }

        // Only now is it safe to swap the contract code.
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());

        env.events().publish(
            (UPGRADE_TOPIC,),
            EventContractUpgraded {
                new_wasm_hash,
                schema_version: SCHEMA_VERSION,
            },
        );

        Ok(())
    }

    /// Emergency admin revocation, callable only by the guardian.
    ///
    /// Per `docs/adr/0004-guardian-emergency-admin-revocation.md`, the guardian
    /// may revoke a compromised admin and replace it with `new_admin`. This
    /// entry point is gated so it can *only* revoke/replace the admin — it
    /// cannot grant the guardian any other admin powers. The guardian's
    /// `require_auth()` is enforced before any storage mutation.
    ///
    /// On success the stored admin is replaced immediately (so every
    /// admin-gated entry point, e.g. [`upgrade`](Self::upgrade), respects the
    /// new admin on the very next call) and `EventGuardianAdminRevoked` is
    /// emitted under the distinct `guardian_revoke` topic.
    pub fn guardian_revoke_admin(
        env: Env,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        // Authenticate the guardian before any other work.
        let guardian: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Guardian)
            .ok_or(ContractError::Unauthorized)?;
        guardian.require_auth();

        // Capture the outgoing admin for the event (may be absent if already
        // revoked); the guardian still may install a replacement.
        let old_admin: Option<Address> = env.storage().instance().get(&StorageKey::Admin);

        // Revoke/replace only: overwrite the admin slot. No other admin powers
        // are granted to the guardian by this entry point.
        env.storage()
            .instance()
            .set(&StorageKey::Admin, &new_admin);

        env.events().publish(
            (GUARDIAN_REVOKE_TOPIC,),
            EventGuardianAdminRevoked {
                old_admin,
                new_admin,
            },
        );

        Ok(())
    }

    // ── Read-only queries ─────────────────────────────────────────────────────

    /// Returns the treasury's current accrued-and-unwithdrawn balance.
    ///
    /// This is a passthrough read of the treasury balance stored by the
    /// accrual flow; no business logic is applied here.
    pub fn treasury_balance(env: Env) -> i128 {
        storage::get_treasury_balance(&env)
    }

    /// Returns the treasury's current configuration.
    ///
    /// Exposes `base_fee_bps`, `treasury_epoch_cap`, and
    /// `treasury_epoch_length` as stored by the configuration flow.
    pub fn treasury_config(env: Env) -> TreasuryConfig {
        storage::get_treasury_config(&env)
    }

    /// Returns the current in-flight two-party withdrawal proposal, if any.
    ///
    /// Returns `None` cleanly when there is no pending proposal rather than
    /// erroring, so callers can poll this query safely.
    pub fn pending_withdrawal(env: Env) -> Option<Withdrawal> {
        storage::get_pending_withdrawal(&env)
    }

    /// Returns the dispute record for `tx_id`, if one exists.
    ///
    /// Unauthenticated read-only query. Returns `None` cleanly when `tx_id`
    /// has never been disputed rather than erroring, so callers can poll this
    /// query safely.
    pub fn get_dispute(env: Env, tx_id: u64) -> Option<Dispute> {
        storage::get_dispute(&env, tx_id)
    }

    /// Returns all currently-unresolved disputes.
    ///
    /// Unauthenticated read-only bulk query intended for reviewer/admin
    /// dashboards polling for work. Disputes cleared by `resolve_dispute` are
    /// excluded, so the result only ever contains unresolved disputes.
    pub fn get_dispute_queue(env: Env) -> Vec<Dispute> {
        storage::get_dispute_queue(&env)
    }
}

// Re-export the admin error type for callers.
pub use admin::AdminError as ContractError;

/// Compile-time guard that `ContractError` stays within Soroban's 50-variant cap.
///
/// `ContractError::__VARIANT_COUNT` is a `const` emitted alongside the enum in
/// `src/types.rs`; if a future PR adds a variant past the ceiling this module
/// fails to compile, so the cap cannot silently regress.
const _: () = assert!(
    crate::types::ContractError::__VARIANT_COUNT <= MAX_CONTRACT_ERROR_VARIANTS,
    "ContractError exceeds Soroban's 50-variant cap; see DECISIONS.md 'Error-code budget'"
);
