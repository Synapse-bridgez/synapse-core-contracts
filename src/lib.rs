#![no_std]

//! # Vault Contract
//!
//! A Soroban smart contract implementing a multi-signer vault with
//! threshold-based transaction approval, signer rotation, admin transfer,
//! treasury withdrawals, and recovery features.

mod admin;
mod callbacks;
mod errors;
mod events;
mod multisig;
mod recovery;
mod storage;
mod treasury;
mod types;
mod validation;

#[cfg(test)]
mod test_admin;
#[cfg(test)]
mod test_callbacks;
#[cfg(test)]
mod test_multisig;
#[cfg(test)]
mod test_recovery;
#[cfg(test)]
mod test_rotation_chaos;
#[cfg(test)]
mod test_treasury;
#[cfg(test)]
mod test_upgrade_safety;

pub use errors::Error;
pub use types::{
    Callback, Config, Proposal, ProposalStatus, Signer, SignerKind, Transaction, TxStatus,
};

use soroban_sdk::{contract, contractimpl, Address, Bytes, Env, Vec};

/// The main vault contract entry point.
#[contract]
pub struct VaultContract;

#[contractimpl]
impl VaultContract {
    /// Initialize the vault with an admin, signer set, and approval threshold.
    pub fn initialize(
        env: Env,
        admin: Address,
        signers: Vec<Address>,
        threshold: u32,
    ) -> Result<(), Error> {
        admin::initialize(&env, admin, signers, threshold)
    }

    /// Return the current admin address.
    pub fn get_admin(env: Env) -> Result<Address, Error> {
        admin::get_admin(&env)
    }

    /// Begin an admin transfer to `new_admin` (issue #5).
    pub fn transfer_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), Error> {
        admin::transfer_admin(&env, current_admin, new_admin)
    }

    /// Accept a pending admin transfer as the nominated new admin.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), Error> {
        admin::accept_admin(&env, new_admin)
    }

    /// Register a callback for a given event kind.
    pub fn register_callback(
        env: Env,
        signer: Address,
        callback: Callback,
    ) -> Result<(), Error> {
        callbacks::register_callback(&env, signer, callback)
    }

    /// Submit a new transaction for signer approval.
    pub fn submit_transaction(
        env: Env,
        proposer: Address,
        target: Address,
        data: Bytes,
    ) -> Result<u64, Error> {
        multisig::submit_transaction(&env, proposer, target, data)
    }

    /// Approve an in-flight transaction as a signer.
    pub fn approve_transaction(env: Env, signer: Address, tx_id: u64) -> Result<(), Error> {
        multisig::approve_transaction(&env, signer, tx_id)
    }

    /// Cancel an in-flight transaction (issue #60).
    pub fn cancel_transaction(env: Env, signer: Address, tx_id: u64) -> Result<(), Error> {
        multisig::cancel_transaction(&env, signer, tx_id)
    }

    /// Retry a failed transaction (issue #61).
    pub fn retry_transaction(env: Env, signer: Address, tx_id: u64) -> Result<u64, Error> {
        multisig::retry_transaction(&env, signer, tx_id)
    }

    /// Start processing an approved transaction.
    pub fn start_processing(env: Env, signer: Address, tx_id: u64) -> Result<(), Error> {
        multisig::start_processing(&env, signer, tx_id)
    }

    /// Begin a timelocked signer rotation (issue #87).
    pub fn begin_rotation(
        env: Env,
        admin: Address,
        new_signers: Vec<Address>,
        new_threshold: u32,
    ) -> Result<(), Error> {
        admin::begin_rotation(&env, admin, new_signers, new_threshold)
    }

    /// Finalize a timelocked signer rotation once the timelock elapses.
    pub fn finalize_rotation(env: Env, admin: Address) -> Result<(), Error> {
        admin::finalize_rotation(&env, admin)
    }

    /// Withdraw funds from the treasury (issue #72).
    pub fn withdraw_treasury(
        env: Env,
        admin: Address,
        recipient: Address,
        amount: i128,
    ) -> Result<(), Error> {
        treasury::withdraw_treasury(&env, admin, recipient, amount)
    }

    /// Pause the contract, blocking state-changing operations.
    pub fn pause(env: Env, admin: Address) -> Result<(), Error> {
        admin::pause(&env, admin)
    }

    /// Unpause the contract.
    pub fn unpause(env: Env, admin: Address) -> Result<(), Error> {
        admin::unpause(&env, admin)
    }
}
