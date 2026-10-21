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
