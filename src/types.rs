//! Shared contract types.
//!
//! Restored as an empty-but-compiling module so `src/lib.rs` can declare
//! `mod types;`. Concrete types are added by later issues in this wave.

use soroban_sdk::contracterror;

/// Errors returned by the contract's entry points.
#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    /// Returned when `initialize` is called on an already-initialized contract.
    AlreadyInitialized = 1,
}
