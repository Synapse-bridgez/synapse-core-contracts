#![no_std]

//! Soroban smart contract crate.
//!
//! This crate exposes the contract entry points and supporting modules.
//! The migration registry lives in [`migration`] and is invoked by the
//! upgrade path before the WASM swap commits.

mod migration;

pub use migration::{MigrationRegistry, MigrationRegistryClient};
