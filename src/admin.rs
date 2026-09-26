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
//! ## Quota configuration
//!
//! The contract bounds abuse along three independent dimensions, each with its
//! own admin-configurable limit and its own distinguishable error so off-chain
//! operators can tell exactly which control triggered:
//!
//! | Dimension                     | Config setter                     | Rejection error                       |
//! |-------------------------------|-----------------------------------|---------------------------------------|
//! | Per-anchor amount ceiling     | `set_anchor_amount_ceiling`       | `ContractError::AnchorAmountCeilingExceeded` |
//! | Per-signer outstanding cap    | `set_signer_outstanding_cap`      | `ContractError::SignerOutstandingCapExceeded` |
//! | Per-anchor storage-cost quota | `set_anchor_storage_quota`        | `ContractError::AnchorStorageQuotaExceeded` |
//!
//! All three setters are admin-only and share the same configuration and
//! error-reporting pattern.

use soroban_sdk::{Address, Env};

use crate::storage::StorageClient;
use crate::types::ContractError;

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

    /// Set the per-anchor storage-cost quota.
    ///
    /// The quota is an *approximate* proxy for the on-ledger storage footprint
    /// attributable to a single anchor/issuer identifier.  It is computed
    /// cheaply as the sum, over the anchor's active (non-terminal) entries, of
    /// `1 + total field length` — i.e. entry count plus the combined byte length
    /// of the variable-length string fields.  This deliberately does **not**
    /// meter exact ledger byte cost (key encoding, XDR framing, rent curve, and
    /// per-entry overhead are ignored); it is a monotonic, cheaply-computable
    /// bound that closes the adjacent gap left by the per-signer outstanding
    /// cap.  Quota is released as entries move to terminal/archived states.
    ///
    /// Admin-only.  Shares the configuration and error-reporting pattern of the
    /// per-anchor amount ceiling and per-signer outstanding cap.
    pub fn set_anchor_storage_quota(env: &Env, quota: u32) -> Result<(), ContractError> {
        Self::require_admin(env)?;
        StorageClient::set_anchor_storage_quota(env, quota);
        Ok(())
    }

    /// Read the configured per-anchor storage-cost quota.
    ///
    /// Returns the approximate storage-cost proxy ceiling enforced inside
    /// `register_callback`.  See [`AdminClient::set_anchor_storage_quota`] for
    /// the proxy definition and its documented accuracy limits.
    pub fn get_anchor_storage_quota(env: &Env) -> u32 {
        StorageClient::get_anchor_storage_quota(env)
    }
}
