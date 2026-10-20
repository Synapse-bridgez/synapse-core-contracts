//! Storage helpers for the contract.
//!
//! This module centralises the storage keys and the small read/write helpers
//! used by the contract entry points. It is intentionally minimal for now so
//! that `src/lib.rs` can declare `mod storage;` and compile cleanly; entry-point
//! issues in this wave add the concrete accessors they need.

use soroban_sdk::{contracttype, Address, Env};

/// Storage keys used by the contract.
///
/// Kept as a single enum so that every entry point shares one namespaced key
/// space and later issues can extend it without touching `lib.rs`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Contract administrator address.
    Admin,
    /// Relay signer address authorised to submit relayed operations.
    RelaySigner,
    /// Whether the contract has been initialised.
    Initialized,
}

/// Returns whether the contract has been initialised.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
///
/// # Returns
///
/// `true` if the `Initialized` flag is set in instance storage.
#[must_use]
pub fn is_initialized(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Initialized)
        .unwrap_or(false)
}

/// Marks the contract as initialised in instance storage.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
pub fn set_initialized(env: &Env) {
    env.storage()
        .instance()
        .set(&DataKey::Initialized, &true);
}

/// Stores the contract administrator address in instance storage.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
/// * `admin` - The administrator address to persist.
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

/// Returns the stored contract administrator address, if any.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
///
/// # Returns
///
/// `Some(admin)` when the contract has been initialised, otherwise `None`.
#[must_use]
pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::Admin)
}

/// Stores the relay signer address in instance storage.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
/// * `relay_signer` - The relay signer address to persist.
pub fn set_relay_signer(env: &Env, relay_signer: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::RelaySigner, relay_signer);
}

/// Returns the stored relay signer address, if any.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
///
/// # Returns
///
/// `Some(relay_signer)` when the contract has been initialised, otherwise `None`.
#[must_use]
pub fn get_relay_signer(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::RelaySigner)
}
