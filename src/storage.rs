//! Storage helpers for the contract.
//!
//! Centralises the storage keys and accessors used across the contract so
//! that the auth, upgrade, and migration flows all read/write the same
//! locations. See `docs/adr/0004-timelocked-upgrade.md` for the timelocked
//! upgrade state machine.
//!
//! This module also owns the pack/unpack conversion between the public
//! [`Transaction`] type and the packed [`StoredTransaction`] form persisted to
//! ledger storage, and centralises the storage keys and small read/write
//! helpers used by the contract entry points.

use crate::events;
use crate::types::{
    CallbackType, DataKey, Error, PendingUpgrade, StoredTransaction, Transaction,
    TransactionStatus, UpgradeRecord,
};
use soroban_sdk::{contracttype, Address, BytesN, Env, Vec};

/// Storage key for the in-flight timelocked upgrade proposal (issue #100).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpgradeKey {
    Pending,
    History,
    SchemaRange,
    UpgradeDelay,
}

/// Default minimum schema version when no compatibility range is configured.
///
/// A permissive default (0) is chosen so that an unconfigured contract does not
/// silently reject otherwise-valid upgrades; admins should set an explicit
/// range via `set_schema_compatibility_range` (issue #104) to tighten it.
pub const DEFAULT_SCHEMA_MIN: u32 = 0;

/// Default maximum schema version when no compatibility range is configured.
///
/// `u32::MAX` keeps the default range permissive (accept any schema) so that
/// upgrades are not blocked before an admin configures a range.
pub const DEFAULT_SCHEMA_MAX: u32 = u32::MAX;

/// Default upgrade timelock delay (in ledgers) when none is configured.
///
/// Zero means no enforced delay until an admin sets one via
/// `set_upgrade_delay` (issue #105).
pub const DEFAULT_UPGRADE_DELAY: u32 = 0;

/// Maximum number of transactions a single `get_transactions_by_status` page
/// may return. Requests with `limit == 0` or `limit > MAX_STATUS_PAGE_LIMIT`
/// are rejected with [`Error::InvalidLimit`].
///
/// Bounding the page size keeps the read O(page-size) against the slot-based
/// status index (issue #24) and prevents callers from forcing an unbounded
/// scan of the index in a single invocation.
pub const MAX_STATUS_PAGE_LIMIT: u32 = 100;

/// Bit offset of the `status` field within `StoredTransaction::packed`.
const STATUS_SHIFT: u32 = 0;
/// Bit mask for the `status` field (4 bits).
const STATUS_MASK: u32 = 0xF;

/// Bit offset of the `callback_type` field within `StoredTransaction::packed`.
const CALLBACK_SHIFT: u32 = 4;
/// Bit mask for the `callback_type` field (4 bits).
const CALLBACK_MASK: u32 = 0xF;

/// Bit offset of the `retry_count` field within `StoredTransaction::packed`.
const RETRY_SHIFT: u32 = 8;
/// Bit mask for the `retry_count` field (4 bits).
///
/// Sized generously (4 bits, max 15) relative to the original wave 9 design
/// intent of a max-retry constant of 3, so a future increase does not require
/// a breaking re-pack of the stored representation.
const RETRY_MASK: u32 = 0xF;

/// Map a `u32` discriminant back to a [`TransactionStatus`].
fn status_from_u32(value: u32) -> TransactionStatus {
    match value {
        1 => TransactionStatus::Completed,
        2 => TransactionStatus::Failed,
        3 => TransactionStatus::Retrying,
        4 => TransactionStatus::Cancelled,
        _ => TransactionStatus::Pending,
    }
}

/// Map a `u32` discriminant back to a [`CallbackType`].
fn callback_from_u32(value: u32) -> CallbackType {
    match value {
        1 => CallbackType::OnSuccess,
        2 => CallbackType::OnFailure,
        3 => CallbackType::OnComplete,
        _ => CallbackType::None,
    }
}

/// Pack a [`Transaction`] into its on-ledger [`StoredTransaction`] form.
pub fn pack(tx: &Transaction) -> StoredTransaction {
    let packed = ((tx.status as u32 & STATUS_MASK) << STATUS_SHIFT)
        | ((tx.callback_type as u32 & CALLBACK_MASK) << CALLBACK_SHIFT)
        | ((tx.retry_count & RETRY_MASK) << RETRY_SHIFT);

    StoredTransaction {
        tx_id: tx.tx_id.clone(),
        packed,
        amount: tx.amount,
        stellar_account: tx.stellar_account.clone(),
        asset_code: tx.asset_code.clone(),
        asset_issuer: tx.asset_issuer.clone(),
        tags: tx.tags.clone(),
    }
}

/// Unpack a [`StoredTransaction`] back into the public [`Transaction`] form.
pub fn unpack(stored: &StoredTransaction) -> Transaction {
    let status = status_from_u32((stored.packed >> STATUS_SHIFT) & STATUS_MASK);
    let callback_type = callback_from_u32((stored.packed >> CALLBACK_SHIFT) & CALLBACK_MASK);
    let retry_count = (stored.packed >> RETRY_SHIFT) & RETRY_MASK;

    Transaction {
        tx_id: stored.tx_id.clone(),
        status,
        callback_type,
        amount: stored.amount,
        stellar_account: stored.stellar_account.clone(),
        asset_code: stored.asset_code.clone(),
        asset_issuer: stored.asset_issuer.clone(),
        retry_count,
        tags: stored.tags.clone(),
    }
}

/// Returns the configured upgrade delay in seconds (issue #84/#105).
pub fn upgrade_delay(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::UpgradeDelay)
        .unwrap_or(0)
}

/// Persist the in-flight timelocked upgrade proposal.
pub fn set_pending_upgrade(env: &Env, pending: &PendingUpgrade) {
    let ready_at = env.ledger().timestamp().saturating_add(upgrade_delay(env));
    env.storage()
        .instance()
        .set(&DataKey::PendingUpgrade, pending);
    env.storage()
        .instance()
        .set(&DataKey::UpgradeReadyAt, &ready_at);
}

/// Read the in-flight timelocked upgrade proposal, if any.
pub fn get_pending_upgrade(env: &Env) -> Option<PendingUpgrade> {
    env.storage().instance().get(&DataKey::PendingUpgrade)
}

/// Returns the ledger timestamp at which the pending upgrade becomes
/// finalizable, if a proposal is pending.
pub fn get_upgrade_ready_at(env: &Env) -> Option<u64> {
    env.storage().instance().get(&DataKey::UpgradeReadyAt)
}

/// Clear the in-flight timelocked upgrade proposal (after finalize/cancel).
pub fn clear_pending_upgrade(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingUpgrade);
    env.storage().instance().remove(&DataKey::UpgradeReadyAt);
}

/// Append a completed upgrade to the history backing store (issue #27).
pub fn push_upgrade_history(env: &Env, record: &UpgradeRecord) {
    let mut history: Vec<UpgradeRecord> = env
        .storage()
        .persistent()
        .get(&UpgradeKey::History)
        .unwrap_or_else(|| Vec::new(env));
    history.push_back(record.clone());
    env.storage()
        .persistent()
        .set(&UpgradeKey::History, &history);
}

/// Read the full upgrade history backing store (issue #27).
pub fn get_upgrade_history(env: &Env) -> Vec<UpgradeRecord> {
    env.storage()
        .persistent()
        .get(&UpgradeKey::History)
        .unwrap_or_else(|| Vec::new(env))
}

/// Read the most recently completed upgrade, if any.
pub fn previous_upgrade(env: &Env) -> Option<UpgradeRecord> {
    let history = get_upgrade_history(env);
    history.last()
}

/// Persist the configured schema compatibility range (min, max) (issue #104).
pub fn set_schema_compatibility_range(env: &Env, min: u32, max: u32) {
    env.storage()
        .persistent()
        .set(&UpgradeKey::SchemaRange, &(min, max));
}

/// Read the configured schema compatibility range (min, max) (issue #55/#104).
///
/// Returns `(DEFAULT_SCHEMA_MIN, DEFAULT_SCHEMA_MAX)` when unset, i.e. a
/// permissive range that accepts any schema version.
pub fn get_schema_compatibility_range(env: &Env) -> (u32, u32) {
    env.storage()
        .persistent()
        .get(&UpgradeKey::SchemaRange)
        .unwrap_or((DEFAULT_SCHEMA_MIN, DEFAULT_SCHEMA_MAX))
}

/// Persist the configured upgrade timelock delay in ledgers (issue #105).
pub fn set_upgrade_delay(env: &Env, delay: u32) {
    env.storage()
        .persistent()
        .set(&UpgradeKey::UpgradeDelay, &delay);
}

/// Read the configured upgrade timelock delay in ledgers (issue #105).
///
/// Returns `DEFAULT_UPGRADE_DELAY` (0) when unset, i.e. no enforced delay.
pub fn get_upgrade_delay(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get(&UpgradeKey::UpgradeDelay)
        .unwrap_or(DEFAULT_UPGRADE_DELAY)
}

/// Returns the current schema version of the stored contract state.
pub fn get_schema_version(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::SchemaVersion)
        .unwrap_or(0)
}

/// Persists the schema version of the stored contract state.
pub fn set_schema_version(env: &Env, version: u32) {
    env.storage()
        .instance()
        .set(&DataKey::SchemaVersion, &version);
}

/// Returns whether the contract has been initialised.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
///
/// # Returns
///
/// `true` if the `Initialized` flag is set in instance storage.
#[must_use]
pub fn is_initialized(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Initialized)
        .unwrap_or(false)
}

/// Marks the contract as initialised in instance storage.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
pub fn set_initialized(env: &Env) {
    env.storage()
        .instance()
        .set(&DataKey::Initialized, &true);
}

/// Stores the contract administrator address in instance storage.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
/// * `admin` - The administrator address to persist.
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

/// Returns the stored contract administrator address, if any.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
///
/// # Returns
///
/// `Some(admin)` when the contract has been initialised, otherwise `None`.
#[must_use]
pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

/// Stores the relay signer address in instance storage.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
/// * `relay_signer` - The relay signer address to persist.
pub fn set_relay_signer(env: &Env, relay_signer: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::RelaySigner, relay_signer);
}

/// Returns the stored relay signer address, if any.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
///
/// # Returns
///
/// `Some(relay_signer)` when the contract has been initialised, otherwise `None`.
#[must_use]
pub fn get_relay_signer(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::RelaySigner)
}

/// Returns the current relay-signer set.
///
/// When the contract is still in the single-relay-signer baseline state (issue
/// #6) and no N-of-M set has been materialised yet, this returns a one-of-one
/// set containing the baseline signer, per ADR 0004's migration semantics.
pub fn get_relay_signers(env: &Env) -> Vec<Address> {
    if let Some(signers) = env.storage().instance().get(&DataKey::RelaySigners) {
        return signers;
    }

    // Baseline single-signer state: promote lazily into a one-of-one set.
    let mut signers = Vec::new(env);
    if let Some(signer) = env.storage().instance().get(&DataKey::RelaySigner) {
        signers.push_back(signer);
    }
    signers
}

/// Appends `new_signer` to the N-of-M relay-signer set, admin-gated.
///
/// If the contract is still in the single-relay-signer baseline state, the
/// first call transitions it into a one-of-one N-of-M set (containing the
/// baseline signer) before appending `new_signer`, yielding a one-of-two set.
///
/// Adding a signer that is already present is rejected with
/// [`Error::SignerAlreadyExists`] rather than silently no-op'ing, so callers
/// cannot mistake a duplicate for a successful rotation.
pub fn add_relay_signer(
    env: &Env,
    caller: &Address,
    new_signer: &Address,
) -> Result<(), Error> {
    caller.require_auth();

    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)?;
    if caller != &admin {
        return Err(Error::Unauthorized);
    }

    let mut signers = get_relay_signers(env);

    for existing in signers.iter() {
        if &existing == new_signer {
            return Err(Error::SignerAlreadyExists);
        }
    }

    signers.push_back(new_signer.clone());
    env.storage().instance().set(&DataKey::RelaySigners, &signers);

    events::relay_signer_added(env, new_signer);

    Ok(())
}

/// Returns the quorum threshold for relay-call approvals.
///
/// Defaults to `1` when no threshold has been configured via
/// `set_relay_threshold`, matching the single-relay-signer baseline where one
/// approval is sufficient.
pub fn get_relay_threshold(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::RelayThreshold)
        .unwrap_or(1)
}

/// Returns the set of signers that have approved `call_id` so far.
///
/// Approvals live in `temporary` storage per the README's storage-tier table
/// ("Relay approvals"), so they are cheap to write and are consumed once the
/// quorum threshold is met.
pub fn get_relay_approvals(env: &Env, call_id: u64) -> Vec<Address> {
    env.storage()
        .temporary()
        .get(&DataKey::RelayApprovals(call_id))
        .unwrap_or_else(|| Vec::new(env))
}

/// Records `caller`'s approval of `call_id` and reports whether quorum is met.
///
/// `caller` must be a member of the current relay-signer set; otherwise
/// [`Error::Unauthorized`] is returned. A signer approving the same `call_id`
/// twice does not double-count toward quorum: the duplicate is a no-op and the
/// current approval count is re-evaluated against the threshold.
///
/// Returns `true` exactly when the number of distinct approvals reaches the
/// threshold set via `set_relay_threshold` (exactly-at-threshold succeeds,
/// threshold-minus-one does not). When quorum is met the approval set is
/// consumed from `temporary` storage so the call cannot be replayed.
pub fn approve_relay_call(
    env: &Env,
    call_id: u64,
    caller: &Address,
) -> Result<bool, Error> {
    caller.require_auth();

    let signers = get_relay_signers(env);
    let mut is_signer = false;
    for signer in signers.iter() {
        if &signer == caller {
            is_signer = true;
            break;
        }
    }
    if !is_signer {
        return Err(Error::Unauthorized);
    }

    let mut approvals = get_relay_approvals(env, call_id);

    let mut already_approved = false;
    for approved in approvals.iter() {
        if &approved == caller {
            already_approved = true;
            break;
        }
    }
    if !already_approved {
        approvals.push_back(caller.clone());
    }

    let threshold = get_relay_threshold(env);
    let quorum_met = approvals.len() >= threshold;

    if quorum_met {
        // Consume the approval set once quorum is reached.
        env.storage()
            .temporary()
            .remove(&DataKey::RelayApprovals(call_id));
    } else {
        env.storage()
            .temporary()
            .set(&DataKey::RelayApprovals(call_id), &approvals);
    }

    Ok(quorum_met)
}

/// Returns the slot-based status index for `status` (issue #24).
///
/// The index is a `Vec<u64>` of transaction ids in insertion order, stored
/// under [`DataKey::StatusIndex`]. Reading it is O(1) in the number of
/// transactions; only the requested page is materialised.
fn get_status_index(env: &Env, status: &TransactionStatus) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&DataKey::StatusIndex(status.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

/// Returns a page of transactions currently in `status`, unauthenticated.
///
/// Reads directly from the slot-based status index (issue #24) so the cost is
/// O(page-size) rather than O(total-transactions): only the `limit` ids in
/// `[offset, offset + limit)` are resolved to full [`Transaction`] records.
///
/// # Pagination and consistency
///
/// The index is append-ordered and each call re-reads the current index, so
/// the guaranteed consistency level is **eventually-consistent-per-ledger**:
/// within a single ledger the page is a consistent snapshot, but a transaction
/// that transitions out of `status` between two paginated calls may be absent
/// from (or shifted within) a later page. Callers that need a stable snapshot
/// across pages should pin to a ledger sequence and re-query on mismatch.
///
/// # Errors
///
/// Returns [`Error::InvalidLimit`] when `limit == 0` or
/// `limit > MAX_STATUS_PAGE_LIMIT`, per
/// `test_get_transactions_by_status_rejects_bad_limit`.
pub fn get_transactions_by_status(
    env: &Env,
    status: &TransactionStatus,
    offset: u32,
    limit: u32,
) -> Result<Vec<Transaction>, Error> {
    if limit == 0 || limit > MAX_STATUS_PAGE_LIMIT {
        return Err(Error::InvalidLimit);
    }

    let index = get_status_index(env, status);
    let total = index.len();

    let mut page = Vec::new(env);
    if offset >= total {
        return Ok(page);
    }

    let end = offset.saturating_add(limit).min(total);
    let mut i = offset;
    while i < end {
        let tx_id = index.get_unchecked(i);
        if let Some(tx) = env
            .storage()
            .persistent()
            .get::<DataKey, Transaction>(&DataKey::Transaction(tx_id))
        {
            // Skip entries that have transitioned out of `status` since the
            // index was last compacted; the index is eventually consistent.
            if tx.status == *status {
                page.push_back(tx);
            }
        }
        i += 1;
    }

    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{Address, Bytes, Env, String, Vec};

    #[test]
    fn round_trip_preserves_every_field() {
        let env = Env::default();
        let tx = Transaction {
            tx_id: Bytes::from_array(&env, &[1, 2, 3, 4]),
            status: TransactionStatus::Retrying,
            callback_type: CallbackType::OnFailure,
            amount: 1_000_000,
            stellar_account: Address::generate(&env),
            asset_code: String::from_str(&env, "USDC"),
            asset_issuer: Address::generate(&env),
            retry_count: 3,
            tags: Vec::new(&env),
        };

        let stored = pack(&tx);
        let restored = unpack(&stored);

        assert_eq!(restored, tx);
    }

    #[test]
    fn retry_count_defaults_to_zero_and_round_trips() {
        let env = Env::default();
        let tx = Transaction {
            tx_id: Bytes::from_array(&env, &[9, 9]),
            status: TransactionStatus::Pending,
            callback_type: CallbackType::None,
            amount: 0,
            stellar_account: Address::generate(&env),
            asset_code: String::from_str(&env, "XLM"),
            asset_issuer: Address::generate(&env),
            retry_count: 0,
            tags: Vec::new(&env),
        };

        let stored = pack(&tx);
        assert_eq!((stored.packed >> RETRY_SHIFT) & RETRY_MASK, 0);
        assert_eq!(unpack(&stored).retry_count, 0);
    }

    #[test]
    fn retry_count_is_masked_to_four_bits() {
        let env = Env::default();
        let tx = Transaction {
            tx_id: Bytes::from_array(&env, &[7]),
            status: TransactionStatus::Retrying,
            callback_type: CallbackType::OnComplete,
            amount: 5,
            stellar_account: Address::generate(&env),
            asset_code: String::from_str(&env, "USDC"),
            asset_issuer: Address::generate(&env),
            retry_count: 0xF,
            tags: Vec::new(&env),
        };

        let stored = pack(&tx);
        assert_eq!((stored.packed >> RETRY_SHIFT) & RETRY_MASK, 0xF);
        assert_eq!(unpack(&stored).retry_count, 0xF);
    }
}
}
