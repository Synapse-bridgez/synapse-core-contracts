//! Soroban contract entrypoints.
//!
//! This crate exposes the treasury contract's public interface. Read-only
//! queries live alongside the state-mutating entrypoints and are documented
//! per `CONTRIBUTING.md`.

#![no_std]

use soroban_sdk::{contract, contractimpl, Address, Env};

mod storage;

use storage::{TreasuryConfig, Withdrawal};

#[contract]
pub struct TreasuryContract;

#[contractimpl]
impl TreasuryContract {
    /// Returns the treasury's current accrued-and-unwithdrawn balance.
    ///
    /// This is a passthrough read of the treasury balance stored by the
    /// accrual flow; no business logic is applied here.
    pub fn treasury_balance(env: Env) -> i128 {
        storage::get_treasury_balance(&env)
    }

    /// Returns the treasury's current configuration.
    ///
    /// Exposes `base_fee_bps`, `treasury_epoch_cap`, and
    /// `treasury_epoch_length` as stored by the configuration flow.
    pub fn treasury_config(env: Env) -> TreasuryConfig {
        storage::get_treasury_config(&env)
    }

    /// Returns the current in-flight two-party withdrawal proposal, if any.
    ///
    /// Returns `None` cleanly when there is no pending proposal rather than
    /// erroring, so callers can poll this query safely.
    pub fn pending_withdrawal(env: Env) -> Option<Withdrawal> {
        storage::get_pending_withdrawal(&env)
    }
}
