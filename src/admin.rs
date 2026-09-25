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

    /// Check `nonce` against `addr`'s next expected nonce and increment it.
    ///
    /// Nonces are strictly sequential per address: only the exact next value
    /// is accepted (no gaps, no reuse). Any other value fails with
    /// [`ContractError::InvalidNonce`].
    pub fn consume_nonce(env: &Env, addr: &Address, nonce: u64) -> Result<(), ContractError> {
        let expected = StorageClient::get_nonce(env, addr);
        if nonce != expected {
            return Err(ContractError::InvalidNonce);
        }
        StorageClient::set_nonce(env, addr, expected + 1);
        Ok(())
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
}
