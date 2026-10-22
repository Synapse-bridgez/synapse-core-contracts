// src/storage.rs

use soroban_sdk::{contracttype, Address, BytesN, Env, Symbol};

/// Represents the lifecycle state of a transaction tracked by the contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransactionStatus {
    /// Transaction has been submitted and is awaiting processing.
    Pending,
    /// Transaction is currently being processed by a relayer.
    Processing,
    /// Transaction has been fully completed.
    Completed,
    /// Transaction has been partially completed (see issue #63).
    PartiallyCompleted,
    /// Transaction failed and will not be retried.
    Failed,
}

/// Persistent record describing a single tracked transaction.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transaction {
    /// Unique identifier assigned at submission time.
    pub tx_id: BytesN<32>,
    /// Address that originally submitted the transaction.
    pub submitter: Address,
    /// Current lifecycle status of the transaction.
    pub status: TransactionStatus,
    /// Stellar transaction hash recorded on completion for audit purposes.
    ///
    /// `None` until `complete_transaction` records the hash; afterwards it is
    /// returned by `get_transaction` so off-chain auditors can correlate the
    /// on-chain record with the Stellar ledger entry.
    pub stellar_tx_hash: Option<BytesN<32>>,
    /// Accumulated fee (in stroops) accrued to the treasury for this tx.
    ///
    /// Populated by the fee-accrual hook (issue #73). Remains `0` until that
    /// hook is implemented.
    pub accrued_fee: i128,
}

/// Storage keys used by the contract.
#[contracttype]
pub enum DataKey {
    /// Maps a `BytesN<32>` transaction id to its `Transaction` record.
    Transaction(BytesN<32>),
    /// Global treasury fee accumulator (issue #73).
    TreasuryAccrued,
    /// Admin address authorized to perform privileged operations.
    Admin,
    /// Set of addresses authorized to relay transactions.
    Relayer(Address),
}

/// Returns the stored `Transaction` for `tx_id`, if any.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `tx_id` - The transaction identifier to look up.
///
/// # Returns
/// `Some(Transaction)` when a record exists, otherwise `None`.
pub fn get_transaction(env: &Env, tx_id: &BytesN<32>) -> Option<Transaction> {
    env.storage()
        .persistent()
        .get(&DataKey::Transaction(tx_id.clone()))
}

/// Persists a `Transaction` record, overwriting any existing entry.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `tx` - The transaction record to store.
pub fn put_transaction(env: &Env, tx: &Transaction) {
    env.storage()
        .persistent()
        .set(&DataKey::Transaction(tx.tx_id.clone()), tx);
}

/// Records the Stellar transaction hash on an existing `Transaction` record.
///
/// This is the audit hook used by `complete_transaction`: it persists the
/// `stellar_tx_hash` so subsequent `get_transaction` calls return it, matching
/// the assertions in `test_recovery.rs` and `test_rotation_chaos.rs`.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `tx_id` - The transaction identifier to update.
/// * `stellar_tx_hash` - The Stellar ledger transaction hash to record.
///
/// # Panics
/// Panics if no `Transaction` record exists for `tx_id`.
pub fn set_stellar_tx_hash(env: &Env, tx_id: &BytesN<32>, stellar_tx_hash: &BytesN<32>) {
    let mut tx = get_transaction(env, tx_id)
        .unwrap_or_else(|| panic!("transaction not found"));
    tx.stellar_tx_hash = Some(stellar_tx_hash.clone());
    put_transaction(env, &tx);
}

/// Returns the currently accrued treasury fee total.
///
/// # Arguments
/// * `env` - The contract environment.
///
/// # Returns
/// The accumulated fee in stroops, or `0` if none has been accrued.
pub fn get_treasury_accrued(env: &Env) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::TreasuryAccrued)
        .unwrap_or(0)
}

/// Persists the accrued treasury fee total.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `amount` - The new accumulated fee total in stroops.
pub fn set_treasury_accrued(env: &Env, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::TreasuryAccrued, &amount);
}

/// Fee-accrual hook stub for issue #73.
///
/// Called by `complete_transaction` after the status transition succeeds. When
/// issue #73 lands, this function will compute the fee owed for `tx_id` and add
/// it to the treasury accumulator. For now it is a no-op placeholder so that
/// issue #73 does not require re-opening the `complete_transaction` entry point.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `tx_id` - The transaction that just completed.
pub fn accrue_completion_fee(_env: &Env, _tx_id: &BytesN<32>) {
    // TODO(#73): compute fee for `tx_id` and add to `DataKey::TreasuryAccrued`.
}

/// Returns whether `addr` is an authorized relayer.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `addr` - The address to check.
pub fn is_relayer(env: &Env, addr: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Relayer(addr.clone()))
        .unwrap_or(false)
}

/// Marks `addr` as an authorized relayer.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `addr` - The address to authorize.
pub fn set_relayer(env: &Env, addr: &Address) {
    env.storage()
        .persistent()
        .set(&DataKey::Relayer(addr.clone()), &true);
}

/// Returns the configured admin address, if any.
///
/// # Arguments
/// * `env` - The contract environment.
pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().persistent().get(&DataKey::Admin)
}

/// Persists the admin address.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `admin` - The address to set as admin.
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().persistent().set(&DataKey::Admin, admin);
}

/// Emits a contract event with the given topics and data.
///
/// Thin wrapper kept here so storage/transition helpers can emit without
/// importing the events module directly.
///
/// # Arguments
/// * `env` - The contract environment.
/// * `topics` - The event topics.
/// * `data` - The event payload.
pub fn emit(env: &Env, topics: &soroban_sdk::Vec<Symbol>, data: &soroban_sdk::Val) {
    env.events().publish(topics.clone(), data.clone());
}