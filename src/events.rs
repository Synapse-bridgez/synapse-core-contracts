//! Event definitions and emission helpers for the contract.
//!
//! Events are emitted with a stable topic so off-chain indexers can filter
//! reliably. The `init` topic is emitted exactly once by
//! `initialize(admin, relay_signer)`.
//!
//! The `upgrade` topic is emitted by the admin-gated `upgrade()`
//! entry point after a successful WASM swap (see `DECISIONS.md` §5).
//!
//! The `guardian_revoke` topic is emitted by the guardian emergency admin
//! revocation entry point described in
//! `docs/adr/0004-guardian-emergency-admin-revocation.md`. It is deliberately
//! distinct from the normal `propose_admin`/`accept_admin` transfer events so
//! downstream monitoring can tell a planned handoff apart from an emergency
//! revocation.
//!
//! Relay signer rotation events are emitted under the `relay` topic and mirror
//! the schema in `EVENTS.md`; keep them in sync.
//!
//! Transaction failure events are emitted under the `fail` topic. This is
//! deliberately distinct from the `cancel` topic: `fail` means "the anchor
//! genuinely failed", while `cancel` means "someone decided to stop this".
//!
//! Transaction registration events are emitted under the `reg` topic by
//! `register_callback`; the payload is the newly allocated `tx_id`.
//!
//! Transaction cancellation events are emitted under the `cancel` topic by
//! `cancel_transaction`. The payload is an [`EventTransactionCancelled`]
//! carrying the `tx_id` and the human-readable cancellation reason. `cancel`
//! is a terminal branch alongside `fail` (see README's lifecycle diagram) and
//! is deliberately kept separate so indexers can distinguish an operator
//! decision from an anchor failure.
//!
//! # Guaranteed multi-event emission order
//!
//! Several entry points emit more than one event per invocation. For those
//! entry points the relative order of the emitted events is a documented,
//! tested guarantee (see `EVENTS.md`). The helpers in this module are the
//! single source of truth for those orderings: they publish events in a fixed
//! sequence so the guarantee is enforced at the source rather than relying on
//! call-site discipline.
//!
//! In particular, `complete_transaction` emits `status` **then** `done`; see
//! [`emit_status_then_done`].

use soroban_sdk::{contracttype, symbol_short, Address, BytesN, Env, String, Symbol};

/// Topic symbol for the contract-initialized event ("init" per README).
pub const TOPIC_INIT: Symbol = symbol_short!("init");

/// Topic symbol used for contract-upgrade events.
pub const UPGRADE_TOPIC: Symbol = symbol_short!("upgrade");

/// Topic symbol used for guardian emergency admin revocation events.
pub const GUARDIAN_REVOKE_TOPIC: Symbol = symbol_short!("guardian_revoke");

/// Topic symbol used for relay-related events (matches the "relay" topic in
/// `EVENTS.md`).
pub const RELAY_TOPIC: Symbol = symbol_short!("relay");

/// Topic symbol used for transaction failure events.
///
/// Emitted by `fail_transaction` when an anchor genuinely fails. This is
/// deliberately distinct from the `cancel` topic: `fail` means "the anchor
/// genuinely failed", while `cancel` means "someone decided to stop this".
pub const FAIL_TOPIC: Symbol = symbol_short!("fail");

/// Topic symbol used for transaction cancellation events.
///
/// Emitted by `cancel_transaction` when an operator stops a `Pending` or
/// `Processing` transaction. Deliberately distinct from [`FAIL_TOPIC`].
pub const CANCEL_TOPIC: Symbol = symbol_short!("cancel");

/// Topic symbol used for transaction registration events.
///
/// Emitted by `register_callback` on success (README's live-emitters list).
pub const REG_TOPIC: Symbol = symbol_short!("reg");

/// Topic symbol for the `status` event.
pub const TOPIC_STATUS: &str = "status";
/// Topic symbol for the `done` event.
pub const TOPIC_DONE: &str = "done";

/// Payload emitted when the contract is initialized for the first time.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventContractInitialized {
    /// The admin address stored during initialization.
    pub admin: Address,
    /// The relay signer address stored during initialization.
    pub relay_signer: Address,
}

/// Payload emitted when the contract WASM is upgraded in place.
///
/// `new_wasm_hash` is the hash of the freshly installed WASM and
/// `schema_version` is the live schema version the upgrade was validated
/// against.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventContractUpgraded {
    pub new_wasm_hash: BytesN<32>,
    pub schema_version: u32,
}

/// Payload emitted when a guardian revokes a compromised admin.
///
/// `guardian` is the guardian that triggered the revocation, `revoked_admin`
/// is the admin that was removed, and `new_admin` is the replacement admin
/// that takes effect immediately.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventGuardianAdminRevoked {
    pub guardian: Address,
    pub revoked_admin: Address,
    pub new_admin: Address,
}

/// Payload emitted when the relay signer is rotated.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventRelaySignerRotated {
    /// The relay signer that was active before the rotation.
    pub old_signer: Address,
    /// The relay signer that is active after the rotation.
    pub new_signer: Address,
}

/// Payload emitted when a transaction is cancelled.
///
/// `tx_id` identifies the cancelled transaction and `reason` is the
/// human-readable justification supplied by the caller. The reason is
/// validated against the string-length cap before this event is emitted.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventTransactionCancelled {
    pub tx_id: u64,
    pub reason: String,
}

/// Emit the `EventContractInitialized` event under the "init" topic.
///
/// Called exactly once from `initialize`; the one-time-init guard in the
/// contract ensures this emitter is never reached on a second call.
pub fn emit_contract_initialized(env: &Env, admin: Address, relay_signer: Address) {
    let event = EventContractInitialized {
        admin,
        relay_signer,
    };
    env.events().publish((TOPIC_INIT,), event);
}

/// Emit the `upgrade` event for a successful in-place WASM swap.
///
/// Called by `upgrade()` only after the schema-version guard has passed and
/// `env.deployer().update_current_contract_wasm()` has been invoked.
pub fn emit_contract_upgraded(env: &Env, new_wasm_hash: BytesN<32>, schema_version: u32) {
    let payload = EventContractUpgraded {
        new_wasm_hash,
        schema_version,
    };
    env.events().publish((UPGRADE_TOPIC,), payload);
}

/// Emit the `guardian_revoke` event for a guardian emergency admin revocation.
///
/// Called by the guardian revocation entry point after the admin has been
/// replaced. The dedicated topic keeps this event distinguishable from the
/// normal `propose_admin`/`accept_admin` transfer events.
pub fn emit_guardian_admin_revoked(
    env: &Env,
    guardian: Address,
    revoked_admin: Address,
    new_admin: Address,
) {
    let payload = EventGuardianAdminRevoked {
        guardian,
        revoked_admin,
        new_admin,
    };
    env.events().publish((GUARDIAN_REVOKE_TOPIC,), payload);
}

/// Emit an [`EventRelaySignerRotated`] event under the `relay` topic.
///
/// This is emitted on every successful `set_relay_signer` call, including
/// no-op rotations to the currently-active signer.
pub fn emit_relay_signer_rotated(env: &Env, old_signer: Address, new_signer: Address) {
    let event = EventRelaySignerRotated {
        old_signer,
        new_signer,
    };
    env.events().publish((RELAY_TOPIC,), event);
}

/// Emit the `fail` event for a transaction.
///
/// Publishes the `fail` topic with a payload of `(tx_id, reason)`, allowing
/// off-chain consumers to observe both the affected transaction and the
/// human-readable failure reason.
///
/// # Arguments
///
/// * `env` - The contract environment used to publish the event.
/// * `tx_id` - Identifier of the transaction that failed.
/// * `reason` - Human-readable reason for the failure. Callers are
///   responsible for validating `reason` against the string-length cap
///   (issue #47) before invoking this helper.
pub fn emit_fail(env: &Env, tx_id: u64, reason: &String) {
    env.events()
        .publish((FAIL_TOPIC, tx_id), reason.clone());
}

/// Emit the `reg` event for a newly registered transaction.
///
/// Publishes the `reg` topic with the newly allocated `tx_id` as payload.
/// Called by `register_callback` only after validation, the idempotency check,
/// and the durable write have all succeeded, so a duplicate call that returns
/// the original `tx_id` never emits a second `reg` event.
///
/// # Arguments
///
/// * `env` - The contract environment used to publish the event.
/// * `tx_id` - Identifier allocated for the newly registered transaction.
pub fn emit_reg(env: &Env, tx_id: u64) {
    env.events().publish((REG_TOPIC,), tx_id);
}

/// Emit the `cancel` event for a cancelled transaction.
///
/// Publishes the `cancel` topic with an [`EventTransactionCancelled`] payload
/// carrying the `tx_id` and the cancellation reason. Called by
/// `cancel_transaction` after the state transition to `Cancelled` has been
/// persisted.
///
/// # Arguments
///
/// * `env` - The contract environment used to publish the event.
/// * `tx_id` - Identifier of the cancelled transaction.
/// * `reason` - Human-readable reason for the cancellation. Callers are
///   responsible for validating `reason` against the string-length cap before
///   invoking this helper.
pub fn emit_cancel(env: &Env, tx_id: u64, reason: &String) {
    let payload = EventTransactionCancelled {
        tx_id,
        reason: reason.clone(),
    };
    env.events().publish((CANCEL_TOPIC, tx_id), payload);
}

/// Emit the `status` event.
///
/// Publishes a `status` event with the given `tx_id` and `status` payload.
/// This is the helper used by `start_processing` (`Pending → Processing`) and
/// by the other state-transition entry points. It is a low-level helper;
/// callers that must also emit `done` should use [`emit_status_then_done`] so
/// the documented ordering is preserved.
pub fn emit_status(env: &Env, tx_id: u64, status: u32) {
    let topics = (Symbol::new(env, TOPIC_STATUS), tx_id);
    env.events().publish(topics, status);
}

/// Emit the `done` event.
///
/// Publishes a `done` event with the given `tx_id` and `stellar_tx_hash`
/// payload. This is a low-level helper; callers that must also emit `status`
/// should use [`emit_status_then_done`] so the documented ordering is
/// preserved.
pub fn emit_done(env: &Env, tx_id: u64, stellar_tx_hash: &soroban_sdk::BytesN<32>) {
    let topics = (Symbol::new(env, TOPIC_DONE), tx_id);
    env.events().publish(topics, stellar_tx_hash.clone());
}

/// Emit the `status` event followed by the `done` event, in that exact order.
///
/// This is the guaranteed emission order for `complete_transaction`
/// (`Processing → Completed`): the `status` event is published first, then the
/// `done` event. The ordering is enforced here so every caller inherits it.
///
/// # Arguments
///
/// * `env` - The contract environment.
/// * `tx_id` - The transaction identifier, used as the second topic of both
///   events.
/// * `status` - The new status payload for the `status` event.
/// * `stellar_tx_hash` - The Stellar transaction hash recorded on the `done`
///   event for audit purposes.
pub fn emit_status_then_done(
    env: &Env,
    tx_id: u64,
    status: u32,
    stellar_tx_hash: &soroban_sdk::BytesN<32>,
) {
    emit_status(env, tx_id, status);
    emit_done(env, tx_id, stellar_tx_hash);
}

/// Emit a generic single event with the given topic and payload.
///
/// Convenience wrapper used by entry points that emit a single event and
/// therefore make no ordering claim.
pub fn emit_single<T>(env: &Env, topic: &str, tx_id: u64, payload: T)
where
    T: soroban_sdk::IntoVal<Env, soroban_sdk::Val>,
{
    let topics = (Symbol::new(env, topic), tx_id);
    env.events().publish(topics, payload);
}

/// Emit an event with an `Address` payload (e.g. relay/admin notifications).
///
/// Convenience wrapper for single-event emitters whose payload is an address.
pub fn emit_address(env: &Env, topic: &str, tx_id: u64, payload: &Address) {
    let topics = (Symbol::new(env, topic), tx_id);
    env.events().publish(topics, payload.clone());
}
