//! # Admin
//!
//! Role-based access control helpers.  The contract has two privileged roles:
//!
//! | Role          | Storage key       | Capabilities                              |
//! |---------------|-------------------|-------------------------------------------|
//! | `admin`       | `StorageKey::Admin`       | Propose/accept admin transfer, rotate relay signer, pause/unpause, upgrade / `propose_upgrade` / `finalize_upgrade` / `cancel_upgrade` / `rollback_upgrade` / `upgrade_and_migrate`, schema-range + delay config; also permitted to drive status transitions |
//! | `relay_signer`| `StorageKey::RelaySigner` | Register callbacks, drive status transitions |
//!
//! Both roles are initialised once and can be rotated by the admin.

use soroban_sdk::{Address, Env, Vec};

use crate::storage::StorageClient;
use crate::types::{AdminTransitionKind, AdminTransitionRecord, ContractError};

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
    /// The relay is checked first because it drives nearly every transition;
    /// a relay caller then skips the admin read and its key encoding (#121).
    /// Both keys are written together by `initialize`, so the result —
    /// including `NotInitialised` before init — is the same in either order.
    pub fn assert_is_relay_or_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let relay = StorageClient::get_relay_signer(env)?;
        if caller != &relay && caller != &StorageClient::get_admin(env)? {
            return Err(ContractError::Unauthorised);
        }
        caller.require_auth();
        Ok(())
    }

    /// Assert that `caller` is specifically the relay signer (not the admin).
    ///
    /// Used by `batch_register_callback` — only the relay may ingest callbacks.
    pub fn require_relay_signer(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let relay = StorageClient::get_relay_signer(env)?;
        if caller != &relay {
            return Err(ContractError::NotRelaySigner);
        }
        caller.require_auth();
        Ok(())
    }

    /// Set the contract-wide maximum transaction amount ceiling.
    ///
    /// This is a single, always-on backstop enforced in `register_callback`
    /// independently of any per-anchor ceiling.  Only the admin may call it.
    ///
    /// Returns `Err(ContractError::Unauthorised)` when auth fails.
    pub fn set_global_max_amount(env: &Env, amount: i128) -> Result<(), ContractError> {
        Self::require_admin(env)?;
        StorageClient::set_global_max_amount(env, amount);
        Ok(())
    }

    /// Read the contract-wide maximum transaction amount ceiling.
    ///
    /// Returns `None` when no global ceiling has been configured.
    pub fn get_global_max_amount(env: &Env) -> Option<i128> {
        StorageClient::get_global_max_amount(env)
    }

    /// Append a record of an admin transition to the append-only history log.
    ///
    /// Called by every admin-transition mechanism (routine two-step transfer
    /// and break-glass guardian-quorum revocation) so that the full provenance
    /// chain of the admin role is preserved on-chain.  `previous_admin` is the
    /// admin being replaced, `new_admin` is the incoming admin, and `kind`
    /// distinguishes *how* the transition happened so routine transfers and
    /// emergency revocations remain forensically distinguishable.
    pub fn record_transition(
        env: &Env,
        previous_admin: &Address,
        new_admin: &Address,
        kind: AdminTransitionKind,
    ) {
        let record = AdminTransitionRecord {
            previous_admin: previous_admin.clone(),
            new_admin: new_admin.clone(),
            kind,
            timestamp: env.ledger().timestamp(),
        };
        StorageClient::append_admin_transition(env, &record);
    }

    /// Return the complete, append-only history of admin transitions since
    /// `initialize()`.
    ///
    /// Each [`AdminTransitionRecord`] captures the prior admin, the incoming
    /// admin, the transition mechanism used (routine two-step transfer vs.
    /// break-glass guardian-quorum revocation), and the ledger timestamp at
    /// which the transition occurred.  Records are returned in chronological
    /// order (oldest first).  Pre-feature transitions are not backfilled; see
    /// `CHANGELOG.md`.
    pub fn get_admin_history(env: &Env) -> Vec<AdminTransitionRecord> {
        StorageClient::get_admin_history(env)
    }
    }
}
