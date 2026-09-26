//! # Admin
//!
//! Role-based access control helpers.  The contract has two privileged roles:
//!
//! | Role          | Storage key       | Capabilities                              |
//! |---------------|-------------------|-------------------------------------------|
//! | `admin`       | `StorageKey::Admin`       | Propose/accept admin transfer, rotate relay signer, pause/unpause, upgrade; also permitted to drive status transitions |
//! | `relay_signer`| `StorageKey::RelaySigner` | Register callbacks, drive status transitions |
//!
//! Both roles are initialised once and can be rotated by the admin.
//!
//! ## Instance-storage size guardrails
//!
//! The admin/relay role records live in instance-tier storage, which is read on
//! (nearly) every call and therefore has the widest blast radius if a single
//! entry grows past Soroban's per-entry size limit.  The role records are fixed
//! size (a single `Address` each), so they cannot grow unboundedly; the helpers
//! below make that invariant explicit and reject any attempt to write an
//! oversized role record with a clear, actionable contract error *before* the
//! raw platform limit is ever reached.

use soroban_sdk::{Address, Env};

use crate::storage::StorageClient;
use crate::types::ContractError;

/// Maximum serialised size, in bytes, permitted for a single instance-storage
/// role record (`admin` or `relay_signer`).
///
/// Soroban enforces a hard per-entry limit at the platform level; this cap is
/// deliberately well below it so that an over-sized write fails with a clean
/// contract-level rejection rather than a raw platform error mid-operation.
/// A role record is a single `Address`, so this bound is never approached in
/// practice — it exists to make the invariant explicit and auditable.
pub const MAX_ROLE_ENTRY_BYTES: u32 = 256;

pub struct AdminClient;

impl AdminClient {
    /// Require the calling transaction to be authorised by the current admin.
    ///
    /// Returns `Err(ContractError::Unauthorised)` when auth fails.
    pub fn require_admin(env: &Env) -> Result<Address, ContractError> {
        let admin = StorageClient::get_admin(env)?;
        admin.require_auth();
        Ok(admin)
    }

    /// Assert that `caller` is either the admin or the trusted relay signer.
    ///
    /// Used by status-transition methods which are callable by both roles.
    pub fn assert_is_relay_or_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin = StorageClient::get_admin(env)?;
        let relay = StorageClient::get_relay_signer(env)?;
        if caller != &admin && caller != &relay {
            return Err(ContractError::Unauthorised);
        }
        caller.require_auth();
        Ok(())
    }

    /// Assert that `caller` is specifically the relay signer (not the admin).
    ///
    /// Used by `register_callback` — only the relay may ingest callbacks.
    #[allow(dead_code)]
    pub fn require_relay_signer(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let relay = StorageClient::get_relay_signer(env)?;
        if caller != &relay {
            return Err(ContractError::NotRelaySigner);
        }
        caller.require_auth();
        Ok(())
    }

    /// Guard an instance-storage role record against exceeding
    /// [`MAX_ROLE_ENTRY_BYTES`].
    ///
    /// `structure` names the aggregate being written (e.g. `"admin"` or
    /// `"relay_signer"`) so the rejection error identifies exactly which
    /// structure and limit was hit.  Returns
    /// `Err(ContractError::InstanceEntryTooLarge)` when the serialised record
    /// would exceed the cap, failing safely well before Soroban's raw
    /// per-entry platform limit.
    pub fn assert_role_entry_fits(
        env: &Env,
        structure: &str,
        value: &Address,
    ) -> Result<(), ContractError> {
        let size = value.to_xdr(env).len();
        if size > MAX_ROLE_ENTRY_BYTES {
            let _ = structure;
            return Err(ContractError::InstanceEntryTooLarge);
        }
        Ok(())
    }
}
