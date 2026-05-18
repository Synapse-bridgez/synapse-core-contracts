//! # Admin
//!
//! Role-based access control helpers.  The contract has two privileged roles:
//!
//! | Role          | Storage key       | Capabilities                              |
//! |---------------|-------------------|-------------------------------------------|
//! | `admin`       | `StorageKey::Admin`       | Transfer admin, rotate relay signer |
//! | `relay_signer`| `StorageKey::RelaySigner` | Register callbacks, drive status transitions |
//!
//! Both roles are initialised once and can be rotated by the admin.

use soroban_sdk::{Address, Env};

use crate::storage::StorageClient;
use crate::types::ContractError;

pub struct AdminClient;

impl AdminClient {
    /// Require the calling transaction to be authorised by the current admin.
    ///
    /// Returns `Err(ContractError::Unauthorised)` when auth fails.
    pub fn require_admin(env: &Env) -> Result<Address, ContractError> {
        // TODO: let admin = StorageClient::get_admin(env)?;
        // TODO: admin.require_auth();
        // TODO: Ok(admin)
        todo!()
    }

    /// Assert that `caller` is either the admin or the trusted relay signer.
    ///
    /// Used by status-transition methods which are callable by both roles.
    pub fn assert_is_relay_or_admin(
        env: &Env,
        caller: &Address,
    ) -> Result<(), ContractError> {
        // TODO: let admin = StorageClient::get_admin(env)?;
        // TODO: let relay = StorageClient::get_relay_signer(env)?;
        // TODO: if caller != &admin && caller != &relay {
        //           return Err(ContractError::Unauthorised);
        //       }
        // TODO: caller.require_auth();
        todo!()
    }

    /// Assert that `caller` is specifically the relay signer (not the admin).
    ///
    /// Used by `register_callback` — only the relay may ingest callbacks.
    pub fn require_relay_signer(env: &Env, caller: &Address) -> Result<(), ContractError> {
        // TODO: let relay = StorageClient::get_relay_signer(env)?;
        // TODO: if caller != &relay { return Err(ContractError::NotRelaySigner) }
        // TODO: caller.require_auth();
        todo!()
    }
}
