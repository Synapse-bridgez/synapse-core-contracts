//! Shared types, constants, and error definitions for the callback registry contract.
//!
//! This module defines the public data structures used across the contract,
//! including the batch-payload validation types that restore the pre-write
//! resource budget gate for `batch_register_callback`, and the transaction
//! lifecycle types used by `cancel_transaction`.

use soroban_sdk::{contracterror, contracttype, Address, String};

/// Maximum number of payloads accepted in a single `batch_register_callback` call.
///
/// Each payload consumes 3 of the 25 write entries a transaction may use, so a
/// batch of 20 could never succeed. `7` is the largest batch size whose write
/// footprint (7 * 3 = 21 entries) stays within the per-transaction budget while
/// leaving headroom for the remaining bookkeeping writes.
pub const MAX_BATCH_SIZE: u32 = 7;

/// Number of write entries a single payload is estimated to consume.
///
/// Used by [`BatchCost`] to project a candidate batch's footprint before any
/// write occurs.
pub const WRITE_ENTRIES_PER_PAYLOAD: u32 = 3;

/// Soroban's per-transaction write-entry budget.
///
/// A transaction may use at most 25 write entries; batch validation must keep
/// the projected footprint at or below this ceiling.
pub const MAX_WRITE_ENTRIES_PER_TX: u32 = 25;

/// Soroban's per-transaction event budget, expressed in bytes.
///
/// Used as the ceiling for [`BatchCost::event_bytes`] during the pre-write
/// budget check.
pub const MAX_EVENT_BYTES_PER_TX: u32 = 16_384;

/// Estimated event bytes emitted per payload.
///
/// Kept private so the estimate stays consistent with the contract's event
/// emission shape.
const EVENT_BYTES_PER_PAYLOAD: u32 = 512;

/// Estimated resource footprint of a candidate batch, computed before any item
/// in the batch is written.
///
/// `BatchCost` is a dry-run estimate: it projects the write and event footprint
/// of a batch so the contract can reject an over-budget batch up front with
/// [`ContractError::BatchResourceBudgetExceeded`], rather than discovering the
/// overrun after a partial write.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchCost {
    /// Projected number of write entries the batch will consume.
    pub write_bytes: u32,
    /// Projected number of event bytes the batch will emit.
    pub event_bytes: u32,
}

impl BatchCost {
    /// Estimate the resource footprint of a batch of `payload_count` payloads.
    ///
    /// The write estimate is `payload_count * WRITE_ENTRIES_PER_PAYLOAD`. The
    /// event estimate is derived from the same count using a fixed per-payload
    /// event size, keeping the estimate deterministic and independent of the
    /// payload contents.
    pub fn estimate(payload_count: u32) -> Self {
        Self {
            write_bytes: payload_count.saturating_mul(WRITE_ENTRIES_PER_PAYLOAD),
            event_bytes: payload_count.saturating_mul(EVENT_BYTES_PER_PAYLOAD),
        }
    }

    /// Returns `true` if this estimate fits within Soroban's per-transaction
    /// write and event budgets.
    pub fn within_budget(&self) -> bool {
        self.write_bytes <= MAX_WRITE_ENTRIES_PER_TX
            && self.event_bytes <= MAX_EVENT_BYTES_PER_TX
    }
}

/// Lifecycle status of a transaction.
///
/// The documented lifecycle (see README) is:
/// `Pending -> Processing -> {Completed | Failed | Cancelled}`.
///
/// `Completed`, `Failed`, and `Cancelled` are terminal states. `Cancelled` is
/// the second terminal branch alongside `Failed`, reachable only from
/// `Pending`/`Processing` via `cancel_transaction`.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionStatus {
    /// The transaction has been created but not yet picked up for processing.
    Pending,
    /// The transaction is actively being processed.
    Processing,
    /// The transaction completed successfully (terminal).
    Completed,
    /// The transaction failed during processing (terminal).
    Failed,
    /// The transaction was cancelled by an admin or its registering relay
    /// signer before reaching a successful or failed terminal state (terminal).
    Cancelled,
}

/// A transaction tracked by the contract.
///
/// Records the registering relay signer so that `cancel_transaction` can scope
/// authorization to the admin or the specific relay signer who registered the
/// transaction (not merely any valid relay signer).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transaction {
    /// Unique identifier of the transaction.
    pub tx_id: u64,
    /// The relay signer that registered this transaction.
    ///
    /// Used by `cancel_transaction` to authorize cancellation by the specific
    /// registering signer (in addition to the admin).
    pub relay: Address,
    /// Current lifecycle status of the transaction.
    pub status: TransactionStatus,
    /// Optional human-readable reason recorded on cancellation.
    pub reason: Option<String>,
}

/// Errors returned by the callback registry contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    /// The batch exceeded the configured batch budget.
    ///
    /// Retained for compatibility with existing batch-budget tests.
    BatchBudgetExceeded = 1,
    /// The supplied batch size was invalid (e.g. zero or above `MAX_BATCH_SIZE`).
    ///
    /// Retained for compatibility with existing batch-budget tests.
    InvalidBatchSize = 2,
    /// The estimated resource footprint of a batch would exceed Soroban's
    /// per-transaction entry budget.
    ///
    /// Emitted by the pre-write budget gate when a [`BatchCost`] estimate does
    /// not satisfy [`BatchCost::within_budget`]. This is the batch-specific
    /// validation path restored for `batch_register_callback`.
    BatchResourceBudgetExceeded = 3,
    /// The transaction is in a terminal state (`Completed` or `Failed`) and
    /// therefore cannot be cancelled.
    ///
    /// Returned by `cancel_transaction` when the transaction has already
    /// reached a successful or failed terminal state.
    CannotCancel = 4,
    /// The transaction has already been cancelled.
    ///
    /// Returned by `cancel_transaction` when the transaction is already in the
    /// `Cancelled` terminal state.
    AlreadyCancelled = 5,
}