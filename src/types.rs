//! Shared types for the transaction recovery contract.
//!
//! These types describe the on-chain transaction records and the auxiliary
//! markers used by the manual recovery tooling (e.g. duplicate merging).

use soroban_sdk::{contracttype, Address, BytesN, Env};

/// Lifecycle status of a transaction record.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TxStatus {
    /// Record created but not yet finalized.
    Pending,
    /// Record finalized/settled; immutable for recovery purposes.
    Completed,
    /// Record was merged into another record (see [`MergedInto`]).
    Merged,
}

/// A single transaction record.
///
/// Merging a duplicate does not destroy this record: the `merged_into` field
/// is populated with the primary transaction id and the status is set to
/// [`TxStatus::Merged`], preserving all original data.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transaction {
    /// Unique transaction id.
    pub id: u64,
    /// Account that owns/created the record.
    pub owner: Address,
    /// Current lifecycle status.
    pub status: TxStatus,
    /// Opaque payload retained verbatim across merges.
    pub data: BytesN<32>,
    /// If this record was merged away, the primary transaction id it points to.
    pub merged_into: Option<u64>,
}

/// Marker stored alongside a duplicate record indicating the primary it was
/// merged into. Kept separate from [`Transaction`] so the original record can
/// remain intact ("keeps data").
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergedInto {
    /// The primary transaction id this record was merged into.
    pub primary_tx_id: u64,
}

/// Storage keys used by the contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageKey {
    /// A transaction record keyed by its id.
    Transaction(u64),
    /// A "merged into" marker keyed by the duplicate's id.
    MergedInto(u64),
}

impl Transaction {
    /// Returns `true` when this record has been settled (`Completed`).
    ///
    /// Settled records must not be merged away.
    pub fn is_settled(&self) -> bool {
        matches!(self.status, TxStatus::Completed)
    }

    /// Returns `true` when this record has already been merged away.
    pub fn is_merged(&self) -> bool {
        matches!(self.status, TxStatus::Merged) || self.merged_into.is_some()
    }
}

/// Convenience helper returning the storage key for a transaction record.
pub fn tx_key(_env: &Env, tx_id: u64) -> StorageKey {
    StorageKey::Transaction(tx_id)
}

/// Convenience helper returning the storage key for a merged-into marker.
pub fn merged_into_key(_env: &Env, tx_id: u64) -> StorageKey {
    StorageKey::MergedInto(tx_id)
}
