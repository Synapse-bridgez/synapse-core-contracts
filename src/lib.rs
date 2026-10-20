#![no_std]

//! Contract skeleton for the Soroban smart contract.
//!
//! This crate root wires together the module layout documented in
//! `README.md` ("Module layout") and exposes the `Contract` type that every
//! entry point in this wave attaches to. It intentionally declares zero entry
//! points; subsequent issues add them incrementally to the `#[contractimpl]`
//! block below.

mod admin;
mod events;
mod storage;
mod types;
mod validation;

use soroban_sdk::{contract, contractimpl, Address, Env};

use crate::types::ContractError;

/// The contract type that all entry points are implemented on.
#[contract]
pub struct Contract;

/// Entry-point scaffolding. Intentionally empty for now; later issues add
/// `pub fn` entry points here rather than rewriting this file.
#[contractimpl]
impl Contract {
    /// One-time contract initialization.
    ///
    /// Stores `admin` and `relay_signer` into instance storage (hot-path tier
    /// per `README.md`) and emits `EventContractInitialized` exactly once.
    ///
    /// # Arguments
    ///
    /// * `admin` - The administrative address for the contract.
    /// * `relay_signer` - The address authorized to sign relayed operations.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError::AlreadyInitialized`] if the contract has
    /// already been initialized, regardless of the arguments supplied.
    pub fn initialize(env: Env, admin: Address, relay_signer: Address) -> Result<(), ContractError> {
        if storage::is_initialized(&env) {
            return Err(ContractError::AlreadyInitialized);
        }

        storage::set_admin(&env, &admin);
        storage::set_relay_signer(&env, &relay_signer);
        storage::set_initialized(&env);

        events::emit_contract_initialized(&env, &admin, &relay_signer);

        Ok(())
    }
}
