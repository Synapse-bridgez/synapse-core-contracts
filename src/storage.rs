//! Storage helpers for the relay-signer set and related contract state.
//!
//! See `docs/adr/0004-relay-signer-set.md` for the N-of-M signer-set model and
//! the migration semantics from the single-relay-signer baseline (issue #6).

use soroban_sdk::{Address, Env, Vec};

use crate::events;
use crate::types::{DataKey, Error, Transaction, TransactionStatus};

/// Maximum number of transactions a single `get_transactions_by_status` page
/// may return. Requests with `limit == 0` or `limit > MAX_STATUS_PAGE_LIMIT`
/// are rejected with [`Error::InvalidLimit`].
///
/// Bounding the page size keeps the read O(page-size) against the slot-based
/// status index (issue #24) and prevents callers from forcing an unbounded
/// scan of the index in a single invocation.
pub const MAX_STATUS_PAGE_LIMIT: u32 = 100;

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
