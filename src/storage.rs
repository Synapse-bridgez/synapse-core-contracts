use soroban_sdk::{contracttype, Address, Env, String, Symbol};

use crate::validation::MAX_REASON_LEN;

/// Lifecycle status of an anchor transaction.
///
/// The `Failed` state is reachable from any non-terminal state (`Pending` or
/// `Processing`) via `fail_transaction`, while `Cancelled` is only reachable
/// from `Pending`/`Processing` via `cancel_transaction`. This asymmetry is
/// deliberate: `Failed` means "the anchor genuinely failed", whereas
/// `Cancelled` means "someone decided to stop this".
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TxStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

impl TxStatus {
    /// Returns `true` if the status is terminal (`Completed`, `Failed`, or
    /// `Cancelled`) and therefore no further transitions are permitted.
    pub fn is_terminal(&self) -> bool {
        matches!(self, TxStatus::Completed | TxStatus::Failed | TxStatus::Cancelled)
    }
}

/// Persisted record for a single anchor transaction.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transaction {
    pub tx_id: Symbol,
    pub status: TxStatus,
    pub caller: Address,
    pub reason: Option<String>,
}

#[contracttype]
pub enum DataKey {
    Transaction(Symbol),
    Admin,
    Relay(Address),
}

/// Reads a transaction by `tx_id`, returning `None` if it does not exist.
pub fn get_transaction(env: &Env, tx_id: &Symbol) -> Option<Transaction> {
    env.storage()
        .persistent()
        .get(&DataKey::Transaction(tx_id.clone()))
}

/// Writes (or overwrites) a transaction record.
pub fn set_transaction(env: &Env, tx: &Transaction) {
    env.storage()
        .persistent()
        .set(&DataKey::Transaction(tx.tx_id.clone()), tx);
}

/// Returns `true` if a transaction with `tx_id` exists.
pub fn has_transaction(env: &Env, tx_id: &Symbol) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::Transaction(tx_id.clone()))
}

/// Shared transition helper (issue #57).
///
/// Validates that the transaction identified by `tx_id` exists and that its
/// current status is one of `valid_sources`, then writes the new `target`
/// status along with the `caller` and optional `reason`.
///
/// This is the single entry point used by every status transition so that
/// terminal-state guards and persistence stay consistent across the contract.
///
/// # Errors
/// Returns `Err(TransitionError::NotFound)` if the transaction does not exist,
/// or `Err(TransitionError::InvalidSource)` if the current status is not in
/// `valid_sources`.
pub fn transition_status(
    env: &Env,
    tx_id: &Symbol,
    valid_sources: &[TxStatus],
    target: TxStatus,
    caller: &Address,
    reason: Option<String>,
) -> Result<Transaction, TransitionError> {
    let mut tx = get_transaction(env, tx_id).ok_or(TransitionError::NotFound)?;

    if !valid_sources.contains(&tx.status) {
        return Err(TransitionError::InvalidSource);
    }

    tx.status = target;
    tx.caller = caller.clone();
    tx.reason = reason;
    set_transaction(env, &tx);

    Ok(tx)
}

/// Error returned by [`transition_status`].
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionError {
    NotFound,
    InvalidSource,
}

/// Marks a transaction as `Failed`.
///
/// `Failed` is reachable from any non-terminal state (`Pending` or
/// `Processing`) — unlike `Cancelled`, which is scoped to `Pending`/
/// `Processing` only. This asymmetry is documented and deliberate:
/// `fail_transaction` means "the anchor genuinely failed", while
/// `cancel_transaction` means "someone decided to stop this".
///
/// `reason` must satisfy issue #47's string-length cap
/// ([`MAX_REASON_LEN`]).
pub fn fail_transaction(
    env: &Env,
    tx_id: &Symbol,
    reason: String,
    caller: &Address,
) -> Result<Transaction, TransitionError> {
    // Enforce issue #47's string-length cap on the failure reason.
    if reason.len() > MAX_REASON_LEN {
        return Err(TransitionError::InvalidSource);
    }

    transition_status(
        env,
        tx_id,
        &[TxStatus::Pending, TxStatus::Processing],
        TxStatus::Failed,
        caller,
        Some(reason),
    )
}