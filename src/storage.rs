//! Storage helpers for the relay-signer set and related contract state.
//!
//! See `docs/adr/0004-relay-signer-set.md` for the N-of-M signer-set model and
//! the migration semantics from the single-relay-signer baseline (issue #6).

use soroban_sdk::{Address, Env, Vec};

use crate::events;
use crate::types::{DataKey, Error};

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
