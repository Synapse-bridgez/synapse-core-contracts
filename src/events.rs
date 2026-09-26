//! # Events
//!
//! Every state transition the contract makes is announced via a typed event
//! so that downstream subscribers (Phase 2 Swap Engine, Phase 3 Cross-Chain
//! Bridge, off-chain indexers) can react without polling.
//!
//! ## Event schema convention
//!
//! Each event is emitted as:
//! ```text
//! topics: [Symbol("synapse"), Symbol("<event_name>")]
//! data:   <typed contracttype struct>
//! ```
//!
//! This two-topic convention is consistent with the Stellar Asset Contract
//! standard and makes event filtering straightforward in Horizon / RPC queries.
//!
//! **Public API:** topic names, payload fields/types/order, and multi-event
//! emission order are versioned for Phase 2 / Phase 3 subscribers. See
//! [`EVENTS.md`](../EVENTS.md) (catalogue + semver) and
//! [`CHANGELOG.md`](../CHANGELOG.md#event-schema).
//!
//! ## Reference decoder
//!
//! [`decode_event`] is a standalone reference decoder covering every
//! catalogued event topic. Subscribers should port this logic rather than
//! re-deriving it from prose — see `docs/event-consumer-guide.md` for
//! topic-filtering guidance and TypeScript porting notes.

use soroban_sdk::{contracttype, symbol_short, Env, String, Symbol, Val, Vec};

use crate::types::TransactionStatus;

// ─── Event data structs ───────────────────────────────────────────────────────

/// Emitted by [`SynapseCoreContract::initialize`].
#[contracttype]
pub struct EventInitialised {
    pub admin: soroban_sdk::Address,
    pub relay_signer: soroban_sdk::Address,
    pub ledger: u32,
}

/// Emitted by [`SynapseCoreContract::register_callback`] when a new
/// [`Transaction`] is persisted for the first time.
#[contracttype]
pub struct EventTransactionRegistered {
    pub tx_id: String,
    pub stellar_account: String,
    pub amount: i128,
    pub asset_code: String,
    pub anchor_transaction_id: String,
    pub ledger: u32,
}

/// Emitted on every status change driven by [`SynapseCoreContract::start_processing`],
/// [`SynapseCoreContract::complete_transaction`], or
/// [`SynapseCoreContract::fail_transaction`].
///
/// Phase 2 listens for `new_status == Completed` to trigger the swap flow.
/// Phase 3 listens for `new_status == Completed` after the swap to initiate bridging.
#[contracttype]
pub struct EventStatusChanged {
    pub tx_id: String,
    pub old_status: TransactionStatus,
    pub new_status: TransactionStatus,
    pub ledger: u32,
}

/// Emitted when a transaction reaches terminal state `Completed`.
/// Carries the confirmed Stellar transaction hash for downstream verification.
#[contracttype]
pub struct EventTransactionCompleted {
    pub tx_id: String,
    pub stellar_tx_hash: String,
    pub ledger: u32,
}

/// Emitted when a transaction reaches terminal state `Failed`.
#[contracttype]
pub struct EventTransactionFailed {
    pub tx_id: String,
    pub reason: String,
    pub ledger: u32,
}

/// Emitted when the admin role is transferred.
#[contracttype]
pub struct EventAdminTransferred {
    pub old_admin: soroban_sdk::Address,
    pub new_admin: soroban_sdk::Address,
    pub ledger: u32,
}

/// Emitted by [`SynapseCoreContract::propose_admin`] when a new admin is
/// nominated. The transfer is not yet effective at this point — see
/// [`EventAdminTransferred`], emitted only once the nominee itself calls
/// `accept_admin` (two-step transfer, THREAT_MODEL.md finding F-03).
#[contracttype]
pub struct EventAdminTransferProposed {
    pub current_admin: soroban_sdk::Address,
    pub proposed_admin: soroban_sdk::Address,
    pub ledger: u32,
}

/// Emitted when the trusted relay signer is rotated.
///
/// Relay-signer compromise lets an attacker register forged callbacks and
/// drive the lifecycle state machine, so off-chain monitoring subscribes to
/// this the same way it does [`EventAdminTransferred`].
#[contracttype]
pub struct EventRelaySignerRotated {
    pub old_signer: soroban_sdk::Address,
    pub new_signer: soroban_sdk::Address,
    pub ledger: u32,
}

/// Emitted by [`SynapseCoreContract::upgrade`] when the contract WASM is
/// replaced in-place.
///
/// Downstream indexers and monitoring tooling subscribe to this to detect
/// unexpected upgrades (potential admin-key compromise).
#[contracttype]
pub struct EventContractUpgraded {
    /// Admin that authorised the upgrade.
    pub admin: soroban_sdk::Address,
    /// The new WASM hash (SHA-256 of the deployed `.wasm`).
    pub new_wasm_hash: soroban_sdk::BytesN<32>,
    pub ledger: u32,
    /// The on-chain schema version that `expected_schema_version` was
    /// checked against before this upgrade proceeded (THREAT_MODEL.md
    /// finding F-04). Additive trailing field — see EVENTS.md semver policy.
    pub schema_version: u32,
}

/// Emitted by [`SynapseCoreContract::pause`] / [`SynapseCoreContract::unpause`]
/// whenever the emergency circuit breaker is toggled.
///
/// Off-chain incident-response tooling subscribes to this so a pause is
/// observable on-chain the moment it happens.
#[contracttype]
pub struct EventPauseToggled {
    /// New pause state: `true` = paused, `false` = unpaused.
    pub paused: bool,
    /// Admin address that performed the toggle.
    pub admin: soroban_sdk::Address,
    pub ledger: u32,
}

/// Emitted once per `batch_register_callback` call, after all per-transaction
/// [`EventTransactionRegistered`] events for that batch have been published.
///
/// This is a compact aggregate signal: subscribers that only need "how many
/// transactions did this one batch call register" can read `batch_size`
/// directly instead of correlating many individual per-transaction events.
/// Detailed per-item data remains available from the per-transaction events;
/// this summary intentionally does not duplicate it.
///
/// **Emission ordering:** within a single batch call the contract emits the
/// per-transaction events first (in batch order), then exactly one
/// `EventBatchProcessed` last. This event is emitted even when the batch size
/// is one, and `batch_size` is always the number of items in the batch.
#[contracttype]
pub struct EventBatchProcessed {
    /// Address that invoked the batch registration.
    pub caller: soroban_sdk::Address,
    /// Number of transactions in the batch (accurate for a batch size of one).
    pub batch_size: u32,
    /// `tx_id` of the first transaction in the batch.
    pub first_tx_id: String,
    /// `tx_id` of the last transaction in the batch.
    pub last_tx_id: String,
    pub ledger: u32,
}

/// Stable discriminant identifying which privileged action a
/// [`EventAdminActionTaken`] audit event describes.
///
/// This enum is the normalized summary layer over the contract's specific
/// admin/relay/guardian events: off-chain monitoring can subscribe to the
/// single `admin_act` topic and switch on `action_type` to catch *any*
/// privileged action without needing to know every current or future
/// specific event type.
///
/// **Stability:** existing variants are never removed or renumbered; new
/// privileged entry points append new variants. Subscribers must treat
/// unknown variants as "some privileged action occurred" rather than
/// erroring.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdminActionType {
    /// [`SynapseCoreContract::initialize`] — initial admin/relay setup.
    Initialize,
    /// [`SynapseCoreContract::propose_admin`] — two-step transfer proposed.
    ProposeAdmin,
    /// [`SynapseCoreContract::accept_admin`] — transfer accepted.
    AcceptAdmin,
    /// [`SynapseCoreContract::rotate_relay_signer`] — relay signer rotated.
    RotateRelaySigner,
    /// [`SynapseCoreContract::upgrade`] — contract WASM replaced.
    Upgrade,
    /// [`SynapseCoreContract::pause`] — circuit breaker engaged.
    Pause,
    /// [`SynapseCoreContract::unpause`] — circuit breaker released.
    Unpause,
}

/// Normalized audit event emitted **alongside** (never instead of) the
/// specific event for every privileged entry point.
///
/// Subscribing to the single `admin_act` topic gives operators a
/// future-proof "alert on any privileged action" feed: `action_type`
/// discriminates the action, `caller` is the address that invoked it, and
/// `target` carries the action's primary subject when one exists (e.g. the
/// new admin for a transfer, the new signer for a rotation, the new WASM
/// hash for an upgrade). Actions with no single subject (e.g. pause) set
/// `target` to `None`.
///
/// The detailed record remains the action's specific event; this is a
/// normalized summary layer on top of it.
#[contracttype]
pub struct EventAdminActionTaken {
    /// Which privileged action was taken.
    pub action_type: AdminActionType,
    /// Address that invoked the privileged entry point.
    pub caller: soroban_sdk::Address,
    /// Primary subject of the action, when one exists.
    pub target: Option<soroban_sdk::Address>,
    pub ledger: u32,
}

// ─── Reference decoder ───────────────────────────────────────────────────────

/// The canonical topic name for every catalogued event, in the order they
/// appear in `EVENTS.md`.
///
/// This is the single source of truth for the second topic symbol of each
/// event. Subscribers should filter on `(Symbol("synapse"), <topic>)` and
/// decode the payload with [`decode_event`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventTopic {
    /// `init` — [`EventInitialised`].
    Initialised,
    /// `reg` — [`EventTransactionRegistered`].
    TransactionRegistered,
    /// `status` — [`EventStatusChanged`].
    StatusChanged,
    /// `completed` — [`EventTransactionCompleted`].
    TransactionCompleted,
    /// `failed` — [`EventTransactionFailed`].
    TransactionFailed,
    /// `admin_xfer` — [`EventAdminTransferred`].
    AdminTransferred,
    /// `admin_prop` — [`EventAdminTransferProposed`].
    AdminTransferProposed,
    /// `relay_rot` — [`EventRelaySignerRotated`].
    RelaySignerRotated,
    /// `upgrade` — [`EventContractUpgraded`].
    ContractUpgraded,
    /// `pause` — [`EventPauseToggled`].
    PauseToggled,
    /// `batch` — [`EventBatchProcessed`].
    BatchProcessed,
    /// `admin_act` — [`EventAdminActionTaken`].
    AdminActionTaken,
}

impl EventTopic {
    /// The second topic symbol emitted for this event.
    pub fn symbol(&self) -> Symbol {
        match self {
            EventTopic::Initialised => symbol_short!("init"),
            EventTopic::TransactionRegistered => symbol_short!("reg"),
            EventTopic::Status

/* … truncated 6679 chars — edit only what you need near the top … */
