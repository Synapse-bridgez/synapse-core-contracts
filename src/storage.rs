//! Storage helpers for the contract.
//!
//! This module centralises the storage keys and the small read/write helpers
//! used by the contract entry points. It is intentionally minimal for now so
//! that `src/lib.rs` can declare `mod storage;` and compile cleanly; entry-point
//! issues in this wave add the concrete accessors they need.

use soroban_sdk::{contracttype, Env};

/// Storage keys used by the contract.
///
/// Kept as a single enum so that every entry point shares one namespaced key
/// space and later issues can extend it without touching `lib.rs`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Contract administrator address.
    Admin,
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
/// `true` if the `Initialized` flag is set in persistent storage.
#[must_use]
pub fn is_initialized(env: &Env) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::Initialized)
        .unwrap_or(false)
}

/// Marks the contract as initialised in persistent storage.
///
/// # Arguments
///
/// * `env` - The Soroban environment.
pub fn set_initialized(env: &Env) {
    env.storage()
        .persistent()
        .set(&DataKey::Initialized, &true);
}
