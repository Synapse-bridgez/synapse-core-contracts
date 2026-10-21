use soroban_sdk::{contracttype, Address, BytesN, String, Vec};

/// Lifecycle state of a transaction registered with the contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransactionStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

/// Maximum number of tags that may be attached to a single transaction.
///
/// Enforced by `add_transaction_tag`; once a transaction holds this many tags
/// further additions are rejected. See issue #51.
pub const MAX_TAGS_PER_TRANSACTION: u32 = 10;

/// A transaction record stored on-chain.
///
/// `amount` is the originally requested amount and is preserved for audit even
/// when the transaction settles for a different value. `settled_amount` records
/// the amount actually settled: it is `None` until the transaction completes,
/// and is `Some(value)` once completed. When `settled_amount` is `Some(v)` and
/// `v != amount`, the transaction was partially completed via
/// `partial_complete_transaction`; when `v == amount` it was fully completed.
///
/// `tags` holds additive operational metadata (e.g. marking a transaction for
/// manual review) managed via `add_transaction_tag` / `get_transaction_tags`.
/// Tags never affect `status` or any other entry point's behavior.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transaction {
    pub tx_id: u64,
    pub caller: Address,
    pub amount: i128,
    pub settled_amount: Option<i128>,
    pub stellar_tx_hash: BytesN<32>,
    pub status: TransactionStatus,
    pub tags: Vec<String>,
    pub created_at: u64,
    pub updated_at: u64,
}
