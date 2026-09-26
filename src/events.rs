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
}

impl EventTopic {
    /// The second topic symbol emitted for this event.
    pub fn symbol(&self) -> Symbol {
        match self {
            EventTopic::Initialised => symbol_short!("init"),
            EventTopic::TransactionRegistered => symbol_short!("reg"),
            EventTopic::StatusChanged => symbol_short!("status"),
            EventTopic::TransactionCompleted => symbol_short!("completed"),
            EventTopic::TransactionFailed => symbol_short!("failed"),
            EventTopic::AdminTransferred => symbol_short!("admin_xfer"),
            EventTopic::AdminTransferProposed => symbol_short!("admin_prop"),
            EventTopic::RelaySignerRotated => symbol_short!("relay_rot"),
            EventTopic::ContractUpgraded => symbol_short!("upgrade"),
            EventTopic::PauseToggled => symbol_short!("pause"),
            EventTopic::BatchProcessed => symbol_short!("batch"),
        }
    }

    /// Every catalogued topic. Kept exhaustive so a new event added to the
    /// catalogue without a decoder entry fails to compile here.
    pub fn all() -> [EventTopic; 11] {
        [
            EventTopic::Initialised,
            EventTopic::TransactionRegistered,
            EventTopic::StatusChanged,
            EventTopic::TransactionCompleted,
            EventTopic::TransactionFailed,
            EventTopic::AdminTransferred,
            EventTopic::AdminTransferProposed,
            EventTopic::RelaySignerRotated,
            EventTopic::ContractUpgraded,
            EventTopic::PauseToggled,
            EventTopic::BatchProcessed,
        ]
    }
}

/// A decoded event: the topic plus its typed payload.
///
/// This is the reference shape subscribers should mirror. Porting to
/// TypeScript is a direct translation of [`decode_event`] — see
/// `docs/event-consumer-guide.md`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodedEvent {
    Initialised(EventInitialised),
    TransactionRegistered(EventTransactionRegistered),
    StatusChanged(EventStatusChanged),
    TransactionCompleted(EventTransactionCompleted),
    TransactionFailed(EventTransactionFailed),
    AdminTransferred(EventAdminTransferred),
    AdminTransferProposed(EventAdminTransferProposed),
    RelaySignerRotated(EventRelaySignerRotated),
    ContractUpgraded(EventContractUpgraded),
    PauseToggled(EventPauseToggled),
    BatchProcessed(EventBatchProcessed),
}

/// Decode a raw event into a [`DecodedEvent`].
///
/// `topics` must be the two-topic tuple `(Symbol("synapse"), <event topic>)`
/// and `data` the single payload value, exactly as delivered by the RPC
/// `getEvents` response. Returns `None` for any event that is not part of the
/// catalogued schema (e.g. a future event this decoder predates), so callers
/// can safely skip unknown topics instead of mis-decoding them.
///
/// This is the reference implementation: it is the only place that maps a
/// topic symbol to a payload type, so subscribers that port it cannot drift
/// from the emitter without a compile error here.
#[allow(clippy::too_many_arguments)]
pub fn decode_event(env: &Env, topics: &Vec<Val>, data: &Val) -> Option<DecodedEvent> {
    if topics.len() != 2 {
        return None;
    }
    let namespace: Symbol = topics.get(0)?.try_into().ok()?;
    if namespace != symbol_short!("synapse") {
        return None;
    }
    let topic: Symbol = topics.get(1)?.try_into().ok()?;

    if topic == EventTopic::Initialised.symbol() {
        Some(DecodedEvent::Initialised(data.clone().try_into().ok()?))
    } else if topic == EventTopic::TransactionRegistered.symbol() {
        Some(DecodedEvent::TransactionRegistered(data.clone().try_into().ok()?))
    } else if topic == EventTopic::StatusChanged.symbol() {
        Some(DecodedEvent::StatusChanged(data.clone().try_into().ok()?))
    } else if topic == EventTopic::TransactionCompleted.symbol() {
        Some(DecodedEvent::TransactionCompleted(data.clone().try_into().ok()?))
    } else if topic == EventTopic::TransactionFailed.symbol() {
        Some(DecodedEvent::TransactionFailed(data.clone().try_into().ok()?))
    } else if topic == EventTopic::AdminTransferred.symbol() {
        Some(DecodedEvent::AdminTransferred(data.clone().try_into().ok()?))
    } else if topic == EventTopic::AdminTransferProposed.symbol() {
        Some(DecodedEvent::AdminTransferProposed(data.clone().try_into().ok()?))
    } else if topic == EventTopic::RelaySignerRotated.symbol() {
        Some(DecodedEvent::RelaySignerRotated(data.clone().try_into().ok()?))
    } else if topic == EventTopic::ContractUpgraded.symbol() {
        Some(DecodedEvent::ContractUpgraded(data.clone().try_into().ok()?))
    } else if topic == EventTopic::PauseToggled.symbol() {
        Some(DecodedEvent::PauseToggled(data.clone().try_into().ok()?))
    } else if topic == EventTopic::BatchProcessed.symbol() {
        Some(DecodedEvent::BatchProcessed(data.clone().try_into().ok()?))
    } else {
        let _ = env;
        None
    }
}

// ─── Emitter ─────────────────────────────────────────────────────────────────

pub struct EventEmitter;

impl EventEmitter {
    /// Emit [`EventInitialised`].
    pub fn initialised(
        env: &Env,
        admin: &soroban_sdk::Address,
        relay_signer: &soroban_sdk::Address,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("init")),
            EventInitialised {
                admin: admin.clone(),
                relay_signer: relay_signer.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventTransactionRegistered`].
    pub fn transaction_registered(env: &Env, tx: &crate::types::Transaction) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("reg")),
            EventTransactionRegistered {
                tx_id: tx.id.clone(),
                stellar_account: tx.stellar_account.clone(),
                amount: tx.amount,
                asset_code: tx.asset_code.clone(),
                anchor_transaction_id: tx.anchor_transaction_id.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventBatchProcessed`].
    ///
    /// Must be called once per batch call, after the per-transaction
    /// [`EventTransactionRegistered`] events for the same batch have been
    /// published, so subscribers observe the summary last.
    pub fn batch_processed(
        env: &Env,
        caller: &soroban_sdk::Address,
        batch_size: u32,
        first_tx_id: &String,
        last_tx_id: &String,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("batch")),
            EventBatchProcessed {
                caller: caller.clone(),
                batch_size,
                first_tx_id: first_tx_id.clone(),
                last_tx_id: last_tx_id.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }
}
