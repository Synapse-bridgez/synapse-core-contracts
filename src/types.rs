//! Shared types, constants, and the contract error enum.
//!
//! This module holds the minimal surface backing issues #469–#472: the
//! `ContractError` enum (with a distinct code per admin guard for #469, a
//! `ZeroAmount` code for #472, and a `TagLimitExceeded` code for #471 whose
//! supplied count/limit travel in an event), the storage keys, and the
//! `PauseState` snapshot returned by the `get_pause_state` query (#470).

use soroban_sdk::{contracterror, contracttype, String};

/// Maximum number of tags that may be attached to a single transaction.
///
/// Enforced by `crate::storage::add_transaction_tag`. When the bound is
/// exceeded the contract emits a `taglimit` event carrying the supplied count
/// and the limit (issue #471).
pub const MAX_TAGS_PER_TRANSACTION: u32 = 10;

/// Storage keys for the minimal contract state.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// The administrator address.
    Admin,
    /// The trusted off-chain relay signer.
    RelaySigner,
    /// The pending admin proposed via two-step transfer.
    PendingAdmin,
    /// The emergency guardian.
    Guardian,
    /// Whether the contract is paused.
    Paused,
    /// The optional pause reason.
    PauseReason,
    /// Whether the contract has been initialised.
    Initialised,
    /// The treasury balance available for withdrawal.
    TreasuryBalance,
    /// A transaction record, keyed by id.
    Transaction(u64),
    /// The tags attached to a transaction, keyed by id.
    TransactionTags(u64),
}

/// Snapshot of the contract's pause state, returned by `get_pause_state`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PauseState {
    /// Whether the contract is currently paused.
    pub paused: bool,
    /// The reason recorded when pause was engaged, if any.
    pub reason: Option<String>,
}

/// A minimal on-chain transaction record.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transaction {
    /// The transaction identifier.
    pub id: u64,
    /// The transferred amount.
    pub amount: i128,
}

/// Contract error codes.
///
/// Every variant is a stable, documented code (see `docs/error-codes.md`).
/// The admin guards in `crate::admin` each return a distinct variant
/// (issue #469) so a caller can tell which precondition failed.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    /// A supplied amount was zero or non-positive (issue #472). Transfers and
    /// treasury withdrawals must carry a positive amount.
    ZeroAmount = 1,
    /// The amount exceeded the resolved ceiling.
    AmountExceedsCeiling = 2,
    /// More than `MAX_TAGS_PER_TRANSACTION` tags were supplied (issue #471).
    /// The supplied count and the limit are emitted in a `taglimit` event.
    TagLimitExceeded = 3,
    /// `require_admin` failed: caller is not the admin (issue #469).
    NotAdmin = 10,
    /// `require_relay_or_admin` failed: caller is neither relay nor admin.
    NotRelayOrAdmin = 11,
    /// `require_pending_admin` failed: caller is not the pending admin.
    NotPendingAdmin = 12,
    /// `require_guardian` failed: caller is not the guardian.
    NotGuardian = 13,
    /// The contract is paused; use `get_pause_state` (issue #470).
    ContractPaused = 20,
    /// A referenced entity already exists.
    AlreadyExists = 21,
    /// A referenced entity was not found.
    NotFound = 22,
    /// The contract has not been initialised.
    NotInitialised = 23,
    /// The treasury balance is too low for the withdrawal.
    InsufficientTreasuryBalance = 24,
}
