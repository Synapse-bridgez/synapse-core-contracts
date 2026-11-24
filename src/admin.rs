//! Role-based access-control guards.
//!
//! Each guard authenticates the caller via `require_auth()` and then verifies a
//! specific role. Per issue #469, every guard returns a *distinct*
//! `ContractError` code so a caller (or assistive tech reading the message)
//! can tell which precondition failed without reading source.

use soroban_sdk::Address;

use crate::types::ContractError;

/// Requires that `caller` is the stored `admin`.
///
/// Returns `ContractError::NotAdmin` when the caller is authenticated but is
/// not the admin.
pub fn require_admin(caller: &Address, admin: &Address) -> Result<(), ContractError> {
    caller.require_auth();
    if caller != admin {
        return Err(ContractError::NotAdmin);
    }
    Ok(())
}

/// Requires that `caller` is the relay signer or the admin.
///
/// Returns `ContractError::NotRelayOrAdmin` when the caller matches neither.
pub fn require_relay_or_admin(
    caller: &Address,
    admin: &Address,
    relay_signer: &Address,
) -> Result<(), ContractError> {
    caller.require_auth();
    if caller != admin && caller != relay_signer {
        return Err(ContractError::NotRelayOrAdmin);
    }
    Ok(())
}

/// Requires that `caller` is the pending admin.
///
/// Returns `ContractError::NotPendingAdmin` otherwise.
pub fn require_pending_admin(
    caller: &Address,
    pending_admin: &Address,
) -> Result<(), ContractError> {
    caller.require_auth();
    if caller != pending_admin {
        return Err(ContractError::NotPendingAdmin);
    }
    Ok(())
}

/// Requires that `caller` is the emergency guardian.
///
/// Returns `ContractError::NotGuardian` otherwise.
pub fn require_guardian(caller: &Address, guardian: &Address) -> Result<(), ContractError> {
    caller.require_auth();
    if caller != guardian {
        return Err(ContractError::NotGuardian);
    }
    Ok(())
}
