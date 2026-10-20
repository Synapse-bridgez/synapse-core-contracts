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

use soroban_sdk::{contract, contractimpl};

/// The contract type that all entry points are implemented on.
#[contract]
pub struct Contract;

/// Entry-point scaffolding. Intentionally empty for now; later issues add
/// `pub fn` entry points here rather than rewriting this file.
#[contractimpl]
impl Contract {}
