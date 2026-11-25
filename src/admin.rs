//! Shared role-based access-control helpers.
//!
//! Every admin-gated entry point delegates here instead of reimplementing its
//! own `require_auth()` logic. One helper per *shape* of check:
//!
//! - [`require_admin`] — admin only.
//! - [`require_relay_or_admin`] — relay signer or admin.
//! - [`require_pending_admin`] — the two-step admin transfer's pending admin.
//! - [`require_guardian`] — the emergency guardian (ADR-0004).
//!
//! The helpers take already-loaded [`Address`] values so they carry no
//! storage-tier assumptions; callers read the stored identities via
//! `storage.rs` and pass them in. Ordering is preserved: the caller is
//! authenticated first (`require_auth()`), then compared against the stored
//! identity, so an unauthenticated caller fails with the guard's own distinct
//! error code — callers can tell exactly which guard failed.
//!
//! ## Distinct error codes (issue #483)
//!
//! Each guard now returns its own error variant rather than the shared
//! [`ContractError::NotAuthorized`]:
//!
//! | Guard                   | Failure error                                |
//! |-------------------------|----------------------------------------------|
//! | [`require_admin`]       | [`ContractError::NotAdmin`]                  |
//! | [`require_relay_or_admin`] | [`ContractError::NotRelayOrAdmin`]        |
//! | [`require_pending_admin`]  | [`ContractError::NotPendingAdmin`]        |
//! | [`require_guardian`]    | [`ContractError::NotGuardian`]               |
//!
//! These variants are documented in `docs/error-codes.md`.

use soroban_sdk::{Address, Env};

use crate::types::ContractError;

/// Requires that `caller` is the stored `admin`.
///
/// Authenticates `caller` via `require_auth()` and then compares it against
/// `admin`. Returns [`ContractError::NotAdmin`] when the caller is
/// authenticated but is not the admin.
///
/// # Errors
/// - [`ContractError::NotAdmin`] — caller is authenticated but is not the admin.
pub fn require_admin(env: &Env, caller: &Address, admin: &Address) -> Result<(), ContractError> {
    caller.require_auth();
    if caller != admin {
        return Err(ContractError::NotAdmin);
    }
    let _ = env;
    Ok(())
}

/// Requires that `caller` is either the stored `relay_signer` or the `admin`.
///
/// Authenticates `caller` exactly once via `require_auth()`, then accepts the
/// call if the caller matches either identity. A caller that is both admin and
/// relay signer passes exactly once. Returns [`ContractError::NotRelayOrAdmin`]
/// when the caller is authenticated but matches neither identity.
///
/// # Errors
/// - [`ContractError::NotRelayOrAdmin`] — caller is neither the relay signer
///   nor the admin.
pub fn require_relay_or_admin(
    env: &Env,
    caller: &Address,
    admin: &Address,
    relay_signer: &Address,
) -> Result<(), ContractError> {
    caller.require_auth();
    if caller != admin && caller != relay_signer {
        return Err(ContractError::NotRelayOrAdmin);
    }
    let _ = env;
    Ok(())
}

/// Requires that `caller` is the stored `pending_admin`.
///
/// Used by the two-step admin transfer's accept step. Authenticates `caller`
/// via `require_auth()` and then compares it against `pending_admin`. Returns
/// [`ContractError::NotPendingAdmin`] when the caller is authenticated but is
/// not the pending admin.
///
/// # Errors
/// - [`ContractError::NotPendingAdmin`] — caller is not the pending admin.
pub fn require_pending_admin(
    env: &Env,
    caller: &Address,
    pending_admin: &Address,
) -> Result<(), ContractError> {
    caller.require_auth();
    if caller != pending_admin {
        return Err(ContractError::NotPendingAdmin);
    }
    let _ = env;
    Ok(())
}

/// Requires that `caller` is the stored emergency `guardian`.
///
/// Used by the guardian emergency admin revocation entry point described in
/// `docs/adr/0004-guardian-emergency-admin-revocation.md`. Authenticates
/// `caller` via `require_auth()` and then compares it against `guardian`.
/// Returns [`ContractError::NotGuardian`] when the caller is authenticated
/// but is not the guardian.
///
/// The guardian role is deliberately narrow: it can only *revoke/replace* the
/// admin in an emergency, never grant itself unrelated admin powers. This
/// helper only proves the caller is the guardian; the entry point that uses it
/// is responsible for restricting the action to revocation.
///
/// # Errors
/// - [`ContractError::NotGuardian`] — caller is not the guardian.
pub fn require_guardian(
    env: &Env,
    caller: &Address,
    guardian: &Address,
) -> Result<(), ContractError> {
    caller.require_auth();
    if caller != guardian {
        return Err(ContractError::NotGuardian);
    }
    let _ = env;
    Ok(())
}
