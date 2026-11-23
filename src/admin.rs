//! Admin authorization helpers.
//!
//! Centralises the `require_auth()`-based guards used by the admin entry
//! points so the trust model is defined in exactly one place.

use soroban_sdk::Address;

use crate::errors::ContractError;
use crate::storage;

/// Require that `caller` is the current admin and has authorised the call.
///
/// Returns [`ContractError::Unauthorized`] if `caller` is not the admin.
/// Panics via `require_auth()` if the caller has not signed.
pub fn require_admin(env: &soroban_sdk::Env, caller: &Address) -> Result<(), ContractError> {
    let admin = storage::get_admin(env);
    if *caller != admin {
        return Err(ContractError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

/// Require that `caller` is the pending admin and has authorised the call.
///
/// Returns [`ContractError::Unauthorized`] if `caller` is not the pending
/// admin, or if `caller` is the current admin (a stale pending value must
/// never let the current admin complete a no-op transfer).
/// Panics via `require_auth()` if the caller has not signed.
pub fn require_pending_admin(
    env: &soroban_sdk::Env,
    caller: &Address,
) -> Result<(), ContractError> {
    let current_admin = storage::get_admin(env);
    if *caller == current_admin {
        return Err(ContractError::Unauthorized);
    }
    let pending = storage::get_pending_admin(env).ok_or(ContractError::Unauthorized)?;
    if *caller != pending {
        return Err(ContractError::Unauthorized);
    }
    caller.require_auth();
    Ok(())
}

/// Reject a proposal that nominates the current admin as the new admin.
///
/// A transfer to the current admin would complete as a no-op and never move
/// the role, so it is rejected up front with the existing admin error code.
pub fn require_new_admin(
    env: &soroban_sdk::Env,
    new_admin: &Address,
) -> Result<(), ContractError> {
    let current_admin = storage::get_admin(env);
    if *new_admin == current_admin {
        return Err(ContractError::Unauthorized);
    }
    Ok(())
}
