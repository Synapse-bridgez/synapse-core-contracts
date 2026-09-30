//! # Admin
//!
//! Role-based access control helpers.  The contract has two privileged roles:
//!
//! | Role          | Storage key       | Capabilities                              |
//! |---------------|-------------------|-------------------------------------------|
//! | `admin`       | `StorageKey::Admin`       | Propose/accept admin transfer, rotate relay signer, pause/unpause, upgrade / propose_upgrade / finalize_upgrade / cancel_upgrade / rollback_upgrade / upgrade_and_migrate, schema-range + delay config; also permitted to drive status transitions |
//! | `relay_signer`| `StorageKey::RelaySigner` | Register callbacks, drive status transitions |
//!
//! Both roles are initialised once and can be rotated by the admin. The relay
//! role can be widened into an N-of-M signer set (`StorageKey::RelaySignerSet`);
//! with `threshold > 1`, co-signers pre-approve via `approve_relay_call`.

use soroban_sdk::{Address, Env, Vec};

use crate::storage::StorageClient;
use crate::types::{ContractError, RelaySignerSet};

/// Ledgers a relay co-signer approval stays valid for (~8 minutes at ~5s/ledger).
pub const RELAY_APPROVAL_WINDOW_LEDGERS: u32 = 100;

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

    /// Assert that `caller` is either the admin or a relay signer, and that
    /// the relay quorum (if an N-of-M set is configured) is met.
    ///
    /// Used by status-transition methods which are callable by both roles.
    /// Checks the primary relay first so the common relay path costs a
    /// single role read.
    pub fn assert_is_relay_or_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let primary = StorageClient::get_relay_signer(env)?;
        if caller != &primary {
            let admin = StorageClient::get_admin(env)?;
            if caller == &admin {
                caller.require_auth();
                return Ok(());
            }
        }
        match StorageClient::get_relay_signer_set_opt(env) {
            // No explicit set: the primary relay is the whole 1-of-1 set.
            None if caller == &primary => {
                caller.require_auth();
                Ok(())
            }
            None => Err(ContractError::Unauthorised),
            Some(set) if set.signers.contains(caller) => {
                caller.require_auth();
                Self::check_quorum(env, &set, Some(caller))
            }
            Some(_) => Err(ContractError::Unauthorised),
        }
    }

    /// Require the relay quorum for an entry point that has no explicit
    /// `caller` argument (`register_callback`). The primary relay signer's
    /// auth is always required; with `threshold > 1` the remaining approvals
    /// must come from [`crate::SynapseCoreContract::approve_relay_call`].
    pub fn require_relay(env: &Env) -> Result<Address, ContractError> {
        let primary = StorageClient::get_relay_signer(env)?;
        primary.require_auth();
        if let Some(set) = StorageClient::get_relay_signer_set_opt(env) {
            Self::check_quorum(env, &set, Some(&primary))?;
        }
        Ok(primary)
    }

    /// Assert that `caller` is a relay signer (not the admin), with quorum.
    ///
    /// Used by `batch_register_callback` — only the relay may ingest callbacks.
    pub fn require_relay_signer(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let primary = StorageClient::get_relay_signer(env)?;
        match StorageClient::get_relay_signer_set_opt(env) {
            None if caller == &primary => {
                caller.require_auth();
                Ok(())
            }
            Some(set) if set.signers.contains(caller) => {
                caller.require_auth();
                Self::check_quorum(env, &set, Some(caller))
            }
            _ => Err(ContractError::NotRelaySigner),
        }
    }

    /// Count `caller` plus every signer holding a fresh approval (within
    /// [`RELAY_APPROVAL_WINDOW_LEDGERS`]); fail with
    /// [`ContractError::QuorumNotMet`] below `threshold`. Approvals are
    /// consumed on success so each one authorises exactly one call.
    fn check_quorum(
        env: &Env,
        set: &RelaySignerSet,
        caller: Option<&Address>,
    ) -> Result<(), ContractError> {
        if set.threshold <= 1 {
            return Ok(());
        }
        let now = env.ledger().sequence();
        let mut approvers = Vec::new(env);
        let mut count = 0u32;
        for s in set.signers.iter() {
            if caller == Some(&s) {
                count += 1;
            } else if StorageClient::get_relay_approval(env, &s)
                .map(|l| now.saturating_sub(l) <= RELAY_APPROVAL_WINDOW_LEDGERS)
                .unwrap_or(false)
            {
                count += 1;
                approvers.push_back(s);
            }
        }
        if count < set.threshold {
            return Err(ContractError::QuorumNotMet);
        }
        for s in approvers.iter() {
            StorageClient::clear_relay_approval(env, &s);
        }
        Ok(())
    }
}
