//! # Soroban contract crate
//!
//! This crate exposes the contract's public entry points, types, and events.
//! Every public item is required to carry a doc-comment; the crate-level
//! `#![deny(missing_docs)]` attribute below turns any gap into a build error,
//! which is enforced in CI as a documentation-coverage gate.

#![deny(missing_docs)]

use soroban_sdk::{contract, contractimpl, Address, Env};

/// Contract type marker for the deployed Soroban contract.
#[contract]
pub struct Contract;

/// Public entry points for the contract.
#[contractimpl]
impl Contract {
    /// Initialize the contract with the given administrator address.
    ///
    /// # Arguments
    /// * `env` - The Soroban execution environment.
    /// * `admin` - The address granted administrative privileges.
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    /// Report signer activity for the given signer address.
    ///
    /// # Arguments
    /// * `env` - The Soroban execution environment.
    /// * `signer` - The signer whose activity is being reported.
    pub fn report_signer_activity(env: Env, signer: Address) {
        signer.require_auth();
        let key = DataKey::SignerActivity(signer.clone());
        let count: u32 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &(count + 1));
    }
}

/// Storage keys used by the contract.
#[soroban_sdk::contracttype]
pub enum DataKey {
    /// Address of the contract administrator.
    Admin,
    /// Per-signer activity counter keyed by signer address.
    SignerActivity(Address),
}
