//! Core data types and error definitions for the escrow contract.
//!
//! This module defines the on-chain data structures used by the contract
//! (transactions, callback payloads, disputes, etc.) as well as the
//! [`ContractError`] enum returned by every fallible entry point.
//!
//! # Error variant budget
//!
//! Issue #15 caps [`ContractError`] at 50 variants. New variants must be
//! coordinated against that budget; the variants added here
//! ([`ContractError::StringTooLong`] and
//! [`ContractError::InvalidSettledAmount`]) are accounted for within it.

use soroban_sdk::{contracterror, contracttype, Address, Bytes, String, Vec};

/// Maximum length, in bytes, of a free-text callback payload field.
///
/// Restored per issue #259 and `COST_MODEL.md` §6 ("reflect enforced caps,
/// not just proposed"). Applies to every free-text field in
/// [`CallbackPayload`].
pub const MAX_CALLBACK_FIELD_LEN: u32 = 256;

/// Maximum length, in bytes, of a dispute reason string.
///
/// Restored per issue #259 and `COST_MODEL.md` §6.
pub const MAX_DISPUTE_REASON_LEN: u32 = 512;

/// Maximum length, in bytes, of a cancellation reason string.
///
/// Restored per issue #259 and `COST_MODEL.md` §6.
pub const MAX_CANCEL_REASON_LEN: u32 = 256;

/// Maximum length, in bytes, of a tag text string.
///
/// Restored per issue #259 and `COST_MODEL.md` §6.
pub const MAX_TAG_TEXT_LEN: u32 = 64;

/// Errors returned by contract entry points.
///
/// Variant discriminants are part of the contract's public ABI and must not
/// be reordered or reused. New variants are appended and coordinated against
/// the 50-variant budget tracked in issue #15.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    /// Generic, unspecified failure.
    Unknown = 0,
    /// Caller is not authorized to perform the requested action.
    Unauthorized = 1,
    /// The referenced transaction does not exist.
    TransactionNotFound = 2,
    /// The transaction is not in a state that permits the requested action.
    InvalidTransactionState = 3,
    /// An amount argument was zero or otherwise out of range.
    InvalidAmount = 4,
    /// The referenced dispute does not exist.
    DisputeNotFound = 5,
    /// A required string argument was empty.
    EmptyString = 6,
    /// A free-text string argument exceeded its enforced length cap.
    ///
    /// Restored per issue #259. Raised by `validation::validate_string_length`
    /// (and its per-field wrappers) before any storage write occurs, keeping
    /// the length guard early in the cheapest-first sequencing of issue #50.
    StringTooLong = 7,
    /// A partial-settlement amount was invalid relative to the original
    /// transaction amount.
    ///
    /// Restored per issue #264. Raised by
    /// `validation::validate_settled_amount` when the settled amount is
    /// zero, negative-equivalent, or greater than the original requested
    /// amount. Backs `partial_complete_transaction` (issue #63).
    InvalidSettledAmount = 8,
}

/// Free-text payload attached to a transaction callback.
///
/// Every field is user-supplied and therefore subject to the length caps
/// restored per issue #259 (see the `MAX_*_LEN` constants above).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallbackPayload {
    /// Short machine-readable label for the callback.
    pub label: String,
    /// Human-readable description of the callback.
    pub description: String,
    /// Opaque reference string (e.g. an off-chain identifier).
    pub reference: String,
    /// Arbitrary tag text associated with the callback.
    pub tag: String,
}

/// A dispute raised against a transaction.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dispute {
    /// Identifier of the disputed transaction.
    pub transaction_id: u64,
    /// Address that raised the dispute.
    pub raised_by: Address,
    /// Free-text reason for the dispute (length-capped).
    pub reason: String,
    /// Ledger timestamp at which the dispute was raised.
    pub raised_at: u64,
    /// Whether the dispute has been resolved.
    pub resolved: bool,
}

/// A record of a transaction cancellation.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cancellation {
    /// Identifier of the cancelled transaction.
    pub transaction_id: u64,
    /// Address that performed the cancellation.
    pub cancelled_by: Address,
    /// Free-text reason for the cancellation (length-capped).
    pub reason: String,
    /// Ledger timestamp at which the cancellation occurred.
    pub cancelled_at: u64,
}

/// A transaction tracked by the escrow contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transaction {
    /// Unique transaction identifier.
    pub id: u64,
    /// Payer address.
    pub from: Address,
    /// Payee address.
    pub to: Address,
    /// Original requested amount.
    pub amount: i128,
    /// Amount settled so far (may be less than `amount` for partial
    /// completions; validated by `validation::validate_settled_amount`).
    pub settled_amount: i128,
    /// Optional callback payload.
    pub callback: Option<CallbackPayload>,
    /// Optional tag text (length-capped).
    pub tag: Option<String>,
    /// Ledger timestamp at which the transaction was created.
    pub created_at: u64,
    /// Current lifecycle status.
    pub status: TransactionStatus,
}

/// Lifecycle status of a [`Transaction`].
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TransactionStatus {
    /// Created but not yet funded.
    Pending = 0,
    /// Funds have been escrowed.
    Funded = 1,
    /// Fully completed and settled.
    Completed = 2,
    /// Partially completed and settled.
    PartiallyCompleted = 3,
    /// Cancelled before completion.
    Cancelled = 4,
    /// Under dispute.
    Disputed = 5,
}

/// Convenience alias for a list of transactions.
pub type TransactionList = Vec<Transaction>;

/// Convenience alias for raw byte payloads.
pub type RawBytes = Bytes;