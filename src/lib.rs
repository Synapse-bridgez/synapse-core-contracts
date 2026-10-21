//! Relay signer rotation with timelock.
//!
//! This module implements the timelocked N-of-M-era relay-signer rotation
//! flow: `propose_relay_signer`, `finalize_relay_signer` /
//! `try_finalize_relay_signer`, and `cancel_relay_signer_change`. It mirrors
//! the propose/finalize/cancel shape already used for admin transfer and
//! upgrades. The immediate single-signer rotation path (`set_relay_signer`)
//! is intentionally out of scope here.

use soroban_sdk::{contractimpl, Address, Env};

use crate::events;
use crate::storage;
use crate::{Contract, ContractArgs, ContractClient, Error};

/// Minimum allowed relay-signer rotation delay, in ledgers.
///
/// A zero (or near-zero) delay would defeat the timelock's purpose entirely,
/// so the configured delay must be at least this many ledgers.
pub const MIN_RELAY_SIGNER_DELAY: u32 = 1;

/// Maximum allowed relay-signer rotation delay, in ledgers.
///
/// Bounds the delay to a sane value so a misconfiguration cannot lock the
/// rotation flow out for an unreasonable amount of time.
pub const MAX_RELAY_SIGNER_DELAY: u32 = 17_280; // ~1 day at 5s ledgers

#[contractimpl]
impl Contract {
    /// Configure the relay-signer rotation timelock delay, in ledgers.
    ///
    /// Gated to the admin (per `docs/adr/0004-relay-signer-set.md`). The value
    /// is stored as a sibling config value to the relay-signer threshold and
    /// is consulted by `propose_relay_signer` to derive the finalization
    /// boundary. The delay must be within
    /// [`MIN_RELAY_SIGNER_DELAY`]..=[`MAX_RELAY_SIGNER_DELAY`]; zero-delay is
    /// rejected because it would defeat the timelock.
    ///
    /// Changing the delay only affects proposals created afterwards; an
    /// already-pending proposal keeps the boundary computed at propose time.
    pub fn set_relay_signer_delay(
        env: Env,
        delay_ledgers: u32,
        caller: Address,
    ) -> Result<(), Error> {
        caller.require_auth();
        let admin = storage::get_admin(&env)?;
        if caller != admin {
            return Err(Error::Unauthorized);
        }

        if delay_ledgers < MIN_RELAY_SIGNER_DELAY || delay_ledgers > MAX_RELAY_SIGNER_DELAY {
            return Err(Error::InvalidDelay);
        }

        storage::set_relay_signer_delay(&env, delay_ledgers);
        Ok(())
    }

    /// Propose a new relay signer, starting the timelock.
    ///
    /// Gated to the admin (per `docs/adr/0004-relay-signer-set.md`). The
    /// pending signer is stored together with the ledger sequence at which
    /// finalization becomes legal, derived from the configured
    /// `set_relay_signer_delay`.
    pub fn propose_relay_signer(
        env: Env,
        new_signer: Address,
        caller: Address,
    ) -> Result<(), Error> {
        caller.require_auth();
        let admin = storage::get_admin(&env)?;
        if caller != admin {
            return Err(Error::Unauthorized);
        }

        let delay = storage::get_relay_signer_delay(&env)?;
        let ready_at = env.ledger().sequence() + delay;

        storage::set_pending_relay_signer(&env, &new_signer, ready_at);
        events::relay_signer_proposed(&env, &caller, &new_signer, ready_at);
        Ok(())
    }

    /// Finalize a pending relay-signer proposal once the timelock has elapsed.
    ///
    /// Rejects if there is no pending proposal, or if the current ledger
    /// sequence is strictly before the configured delay boundary. Succeeds
    /// exactly at the boundary.
    pub fn finalize_relay_signer(env: Env, caller: Address) -> Result<(), Error> {
        caller.require_auth();
        let admin = storage::get_admin(&env)?;
        if caller != admin {
            return Err(Error::Unauthorized);
        }

        let (new_signer, ready_at) = storage::get_pending_relay_signer(&env)?;
        if env.ledger().sequence() < ready_at {
            return Err(Error::TimelockNotElapsed);
        }

        storage::set_relay_signer(&env, &new_signer);
        storage::clear_pending_relay_signer(&env);
        events::relay_signer_finalized(&env, &caller, &new_signer);
        Ok(())
    }

    /// Attempt to finalize a pending relay-signer proposal, returning a
    /// boolean instead of an error on rejection.
    ///
    /// Returns `true` when finalization succeeded, `false` when there is no
    /// pending proposal or the timelock has not yet elapsed.
    pub fn try_finalize_relay_signer(env: Env, caller: Address) -> Result<bool, Error> {
        caller.require_auth();
        let admin = storage::get_admin(&env)?;
        if caller != admin {
            return Err(Error::Unauthorized);
        }

        let pending = storage::get_pending_relay_signer(&env);
        let (new_signer, ready_at) = match pending {
            Ok(p) => p,
            Err(_) => return Ok(false),
        };
        if env.ledger().sequence() < ready_at {
            return Ok(false);
        }

        storage::set_relay_signer(&env, &new_signer);
        storage::clear_pending_relay_signer(&env);
        events::relay_signer_finalized(&env, &caller, &new_signer);
        Ok(true)
    }

    /// Cancel a pending relay-signer proposal before it is finalized.
    ///
    /// Rejects if there is no pending proposal. After cancellation, a
    /// subsequent `finalize_relay_signer` call will reject because no pending
    /// proposal remains.
    pub fn cancel_relay_signer_change(env: Env, caller: Address) -> Result<(), Error> {
        caller.require_auth();
        let admin = storage::get_admin(&env)?;
        if caller != admin {
            return Err(Error::Unauthorized);
        }

        let (new_signer, _ready_at) = storage::get_pending_relay_signer(&env)?;
        storage::clear_pending_relay_signer(&env);
        events::relay_signer_cancelled(&env, &caller, &new_signer);
        Ok(())
    }
}
