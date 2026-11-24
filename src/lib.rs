#![no_std]

//! Synapse Core — minimal contract surface for issues #469–#472.
//!
//! This crate exposes a deliberately minimal entry-point surface implementing
//! the four issues: distinct admin-guard error codes (#469), a read-only
//! pause-state query (#470), a tag-limit rejection that reports the supplied
//! count via an event (#471), and a documented zero-amount error code applied
//! to both the transfer and treasury-withdrawal paths (#472).

use soroban_sdk::{contract, contractimpl, Address, Env, String};

mod admin;
mod events;
mod storage;
mod types;
mod validation;

#[cfg(test)]
mod test_amount_ceiling;
#[cfg(test)]
mod test_boundaries;
#[cfg(test)]
mod test_events_conformance;
#[cfg(test)]
mod test_pause;
#[cfg(test)]
mod tests_auth_adversarial;

use crate::admin::{
    require_admin, require_guardian, require_pending_admin, require_relay_or_admin,
};
use crate::storage as store;
use crate::types::{ContractError, PauseState, Transaction};
use crate::validation::{validate_amount_ceiling, validate_nonzero_amount};

/// Ceiling applied to transfer amounts by `register_callback`.
const MAX_TRANSFER_AMOUNT: i128 = 1_000_000_000;

/// The Synapse Core contract.
#[contract]
pub struct SynapseCoreContract;

#[contractimpl]
#[allow(clippy::needless_pass_by_value)]
impl SynapseCoreContract {
    /// Initialise the contract with an admin, relay signer, and guardian.
    pub fn initialize(env: Env, admin: Address, relay_signer: Address, guardian: Address) {
        store::set_admin(&env, admin);
        store::set_relay_signer(&env, relay_signer);
        store::set_guardian(&env, guardian);
        store::set_paused(&env, false);
        store::set_treasury_balance(&env, 0);
        store::set_initialised(&env);
    }

    /// Replace the relay signer (admin only).
    pub fn set_relay_signer(
        env: Env,
        caller: Address,
        new_relay: Address,
    ) -> Result<(), ContractError> {
        let admin = store::get_admin(&env).ok_or(ContractError::NotInitialised)?;
        require_admin(&caller, &admin)?;
        store::set_relay_signer(&env, new_relay);
        Ok(())
    }

    /// Propose a new admin (admin only); sets the pending admin.
    pub fn propose_admin(
        env: Env,
        caller: Address,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        let admin = store::get_admin(&env).ok_or(ContractError::NotInitialised)?;
        require_admin(&caller, &admin)?;
        store::set_pending_admin(&env, new_admin);
        Ok(())
    }

    /// Accept the pending admin transfer (pending admin only).
    pub fn accept_admin(env: Env, caller: Address) -> Result<(), ContractError> {
        let pending = store::get_pending_admin(&env).ok_or(ContractError::NotFound)?;
        require_pending_admin(&caller, &pending)?;
        store::set_admin(&env, pending);
        store::clear_pending_admin(&env);
        Ok(())
    }

    /// Guardian emergency admin revocation (guardian only).
    pub fn guardian_revoke_admin(
        env: Env,
        caller: Address,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        let guardian = store::get_guardian(&env).ok_or(ContractError::NotInitialised)?;
        require_guardian(&caller, &guardian)?;
        store::set_admin(&env, new_admin);
        Ok(())
    }

    /// Engage the emergency pause (admin only) with an optional reason.
    pub fn pause(env: Env, caller: Address, reason: String) -> Result<(), ContractError> {
        let admin = store::get_admin(&env).ok_or(ContractError::NotInitialised)?;
        require_admin(&caller, &admin)?;
        store::set_paused(&env, true);
        store::set_pause_reason(&env, reason);
        Ok(())
    }

    /// Release the emergency pause (admin only).
    pub fn unpause(env: Env, caller: Address) -> Result<(), ContractError> {
        let admin = store::get_admin(&env).ok_or(ContractError::NotInitialised)?;
        require_admin(&caller, &admin)?;
        store::set_paused(&env, false);
        store::clear_pause_reason(&env);
        Ok(())
    }

    /// Read the current pause state and reason (issue #470).
    ///
    /// Read-only: it does not require admin auth, so a client can discover the
    /// pause state before submitting a call.
    pub fn get_pause_state(env: Env) -> PauseState {
        PauseState {
            paused: store::is_paused(&env),
            reason: store::get_pause_reason(&env),
        }
    }

    /// Register a transfer callback (relay or admin). Rejects zero amounts
    /// (issue #472) and amounts above the ceiling, and rejects while paused.
    pub fn register_callback(
        env: Env,
        caller: Address,
        tx_id: u64,
        amount: i128,
    ) -> Result<(), ContractError> {
        if !store::is_initialised(&env) {
            return Err(ContractError::NotInitialised);
        }
        if store::is_paused(&env) {
            return Err(ContractError::ContractPaused);
        }
        let admin = store::get_admin(&env).ok_or(ContractError::NotInitialised)?;
        let relay = store::get_relay_signer(&env).ok_or(ContractError::NotInitialised)?;
        require_relay_or_admin(&caller, &admin, &relay)?;
        validate_nonzero_amount(amount)?;
        validate_amount_ceiling(amount, MAX_TRANSFER_AMOUNT)?;
        if store::transaction_exists(&env, tx_id) {
            return Err(ContractError::AlreadyExists);
        }
        store::save_transaction(&env, Transaction { id: tx_id, amount });
        Ok(())
    }

    /// Withdraw from the treasury (admin only). Rejects zero amounts
    /// (issue #472) and rejects while paused.
    pub fn withdraw_treasury(env: Env, caller: Address, amount: i128) -> Result<(), ContractError> {
        if !store::is_initialised(&env) {
            return Err(ContractError::NotInitialised);
        }
        if store::is_paused(&env) {
            return Err(ContractError::ContractPaused);
        }
        let admin = store::get_admin(&env).ok_or(ContractError::NotInitialised)?;
        require_admin(&caller, &admin)?;
        validate_nonzero_amount(amount)?;
        let balance = store::get_treasury_balance(&env);
        if amount > balance {
            return Err(ContractError::InsufficientTreasuryBalance);
        }
        store::set_treasury_balance(&env, balance - amount);
        Ok(())
    }

    /// Append a tag to a transaction (relay or admin). Enforces
    /// `MAX_TAGS_PER_TRANSACTION` and emits a `taglimit` event with the supplied
    /// count and the limit on overflow (issue #471).
    pub fn add_transaction_tag(
        env: Env,
        caller: Address,
        tx_id: u64,
        tag: String,
    ) -> Result<(), ContractError> {
        if !store::is_initialised(&env) {
            return Err(ContractError::NotInitialised);
        }
        let admin = store::get_admin(&env).ok_or(ContractError::NotInitialised)?;
        let relay = store::get_relay_signer(&env).ok_or(ContractError::NotInitialised)?;
        require_relay_or_admin(&caller, &admin, &relay)?;
        store::add_transaction_tag(&env, tx_id, tag)
    }
}
