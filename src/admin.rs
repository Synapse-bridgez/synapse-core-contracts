//! # Admin
//!
//! Role-based access control helpers.  The contract has two privileged roles:
//!
//! | Role          | Storage key       | Capabilities                              |
//! |---------------|-------------------|-------------------------------------------|
//! | `admin`       | `DataKey::Admin`  | Propose/accept admin transfer, rotate relay signer, pause/unpause, upgrade, configure upgrade quorum; also permitted to drive status transitions |
//! | `relay_signer`| `DataKey::RelaySigner` | Register callbacks, drive status transitions |
//!
//! Both roles are initialised once and can be rotated by the admin.
//!
//! ## Upgrade quorum (#87)
//!
//! When an [`UpgradeQuorum`] is configured, `upgrade` / `propose_upgrade`
//! require M-of-N co-signatures from the designated member set in addition to
//! admin auth. The quorum is distinct from the base admin key so a single
//! compromised admin cannot unilaterally deploy arbitrary WASM.

use soroban_sdk::{Address, Env, Vec};

use crate::storage::StorageClient;
use crate::types::{ContractError, UpgradeQuorum};

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

    /// Require admin auth, accepting either the namespaced or legacy (schema v1)
    /// admin key so [`crate::SynapseCoreContract::migrate_storage_keys`] can run
    /// against a pre-namespacing deployment (#89).
    pub fn require_admin_allowing_legacy(env: &Env) -> Result<Address, ContractError> {
        if let Ok(admin) = StorageClient::get_admin(env) {
            admin.require_auth();
            return Ok(admin);
        }
        let admin: Address = env
            .storage()
            .persistent()
            .get(&crate::types::LegacyStorageKey::Admin)
            .ok_or(ContractError::NotInitialised)?;
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

    /// Validate an upgrade-quorum configuration before persistence.
    pub fn validate_upgrade_quorum(quorum: &UpgradeQuorum) -> Result<(), ContractError> {
        let n = quorum.members.len();
        if n == 0 || quorum.threshold == 0 || quorum.threshold > n {
            return Err(ContractError::InvalidUpgradeQuorum);
        }
        Ok(())
    }

    /// Require admin auth, then enforce the optional upgrade quorum (#87).
    ///
    /// * Quorum absent → single-admin behaviour (cosigners ignored; must be empty
    ///   is not enforced so callers can pass an empty vec).
    /// * Quorum present → each address in `cosigners` that is a quorum member
    ///   must `require_auth()`. Distinct member auths must reach `threshold`.
    ///   Admin alone is never enough when a quorum is configured.
    pub fn require_upgrade_authorisation(
        env: &Env,
        cosigners: &Vec<Address>,
    ) -> Result<Address, ContractError> {
        let admin = Self::require_admin(env)?;
        let Some(quorum) = StorageClient::get_upgrade_quorum(env) else {
            return Ok(admin);
        };
        Self::collect_cosigner_auths(env, &quorum, cosigners)?;
        Ok(admin)
    }

    /// Count distinct quorum-member auths from `cosigners`, requiring each.
    ///
    /// Returns [`ContractError::InsufficientUpgradeQuorum`] when the count is
    /// below threshold (including the empty / one-short cases).
    pub fn collect_cosigner_auths(
        env: &Env,
        quorum: &UpgradeQuorum,
        cosigners: &Vec<Address>,
    ) -> Result<u32, ContractError> {
        let mut approved: Vec<Address> = Vec::new(env);
        let mut i = 0u32;
        while i < cosigners.len() {
            let c = cosigners.get(i).unwrap();
            if Self::is_quorum_member(quorum, &c) && !Self::vec_contains(&approved, &c) {
                c.require_auth();
                approved.push_back(c);
            }
            i = i.saturating_add(1);
        }
        let count = approved.len();
        if count < quorum.threshold {
            return Err(ContractError::InsufficientUpgradeQuorum);
        }
        Ok(count)
    }

    /// Whether `addr` is in the quorum member set.
    pub fn is_quorum_member(quorum: &UpgradeQuorum, addr: &Address) -> bool {
        let mut i = 0u32;
        while i < quorum.members.len() {
            if quorum.members.get(i).unwrap() == *addr {
                return true;
            }
            i = i.saturating_add(1);
        }
        false
    }

    fn vec_contains(list: &Vec<Address>, addr: &Address) -> bool {
        let mut i = 0u32;
        while i < list.len() {
            if list.get(i).unwrap() == *addr {
                return true;
            }
            i = i.saturating_add(1);
        }
        false
    }

    /// Record `caller`'s approval toward a pending upgrade; returns the new
    /// distinct approval count after requiring caller auth and membership.
    pub fn add_upgrade_approval(env: &Env, caller: &Address) -> Result<u32, ContractError> {
        let quorum = StorageClient::get_upgrade_quorum(env)
            .ok_or(ContractError::InsufficientUpgradeQuorum)?;
        if !Self::is_quorum_member(&quorum, caller) {
            return Err(ContractError::NotUpgradeQuorumMember);
        }
        caller.require_auth();

        let mut approvals = StorageClient::get_upgrade_approvals(env);
        if !Self::vec_contains(&approvals, caller) {
            approvals.push_back(caller.clone());
            StorageClient::set_upgrade_approvals(env, &approvals);
        }
        Ok(approvals.len())
    }
}
