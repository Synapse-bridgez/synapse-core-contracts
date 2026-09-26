//! # Admin
//!
//! Role-based access control helpers.  The contract has three privileged roles:
//!
//! | Role           | Storage key                | Capabilities                              |
//! |----------------|----------------------------|-------------------------------------------|
//! | `admin`        | `StorageKey::Admin`        | Propose/accept/renounce admin, rotate relay/guardian, pause/unpause, upgrade, rate-limit config |
//! | `relay_signer` | `StorageKey::RelaySigner`  | Register callbacks, drive status transitions, vote on auto-unpause, set attestation |
//! | `guardian`     | `StorageKey::Guardian`     | Vote on auto-unpause (and future guardian-pause paths) |
//!
//! ## Admin rate limiting (#75)
//!
//! Privileged admin entry points share a **fixed-window** counter keyed by
//! ledger sequence (not a sliding window). Once `max_per_window` counted calls
//! land inside `[window_start, window_start + window_ledgers)`, further counted
//! calls return [`ContractError::AdminRateLimited`]. When
//! `ledger.sequence() >= window_start + window_ledgers` the counter resets
//! to 1 for the new window.
//!
//! ### Exemptions (never consume budget)
//!
//! - [`crate::SynapseCoreContract::unpause`] — recovery must not be rate-limited
//!   into helplessness after a legitimate pause.
//! - [`crate::SynapseCoreContract::unpause_auto`] — same rationale for auto-pause.
//! - [`crate::SynapseCoreContract::set_admin_rate_limit`] — otherwise a
//!   compromised key could lock the limit and strand operators.
//! - [`crate::SynapseCoreContract::renounce_admin`] — succession must remain
//!   reachable under pressure.
//!
//! Relay-signer and guardian actions are **out of scope** for this limiter.

use soroban_sdk::{Address, Env};

use crate::storage::StorageClient;
use crate::types::{AdminRateLimitConfig, AdminRateLimitState, ContractError};

pub struct AdminClient;

impl AdminClient {
    /// Require the calling transaction to be authorised by the current admin.
    ///
    /// Returns `Err(ContractError::Unauthorised)` when auth fails.
    /// Does **not** apply rate limiting — use [`Self::require_admin_rate_limited`]
    /// for counted privileged entry points.
    pub fn require_admin(env: &Env) -> Result<Address, ContractError> {
        let admin = StorageClient::get_admin(env)?;
        admin.require_auth();
        Ok(admin)
    }

    /// Like [`Self::require_admin`], then consumes one slot of the admin
    /// rate-limit budget when a limit is configured.
    pub fn require_admin_rate_limited(env: &Env) -> Result<Address, ContractError> {
        let admin = Self::require_admin(env)?;
        Self::consume_admin_rate_limit(env)?;
        Ok(admin)
    }

    /// Consume one unit of the fixed-window admin rate-limit budget.
    ///
    /// No-op when no config has been set (limit disabled by default).
    pub fn consume_admin_rate_limit(env: &Env) -> Result<(), ContractError> {
        let Some(config) = StorageClient::get_admin_rate_limit_config(env) else {
            return Ok(());
        };
        let now = env.ledger().sequence();
        let mut state =
            StorageClient::get_admin_rate_limit_state(env).unwrap_or(AdminRateLimitState {
                window_start: now,
                count: 0,
            });

        // Fixed-window reset: once `now` reaches the exclusive end of the
        // current window, open a fresh window starting at `now`.
        if now >= state.window_start.saturating_add(config.window_ledgers) {
            state.window_start = now;
            state.count = 0;
        }

        if state.count >= config.max_per_window {
            return Err(ContractError::AdminRateLimited);
        }

        state.count = state.count.saturating_add(1);
        StorageClient::set_admin_rate_limit_state(env, &state);
        Ok(())
    }

    /// Validate and persist a new admin rate-limit configuration.
    pub fn set_rate_limit_config(
        env: &Env,
        max_per_window: u32,
        window_ledgers: u32,
    ) -> Result<AdminRateLimitConfig, ContractError> {
        if max_per_window == 0 || window_ledgers == 0 {
            return Err(ContractError::InvalidRateLimitConfig);
        }
        let config = AdminRateLimitConfig {
            max_per_window,
            window_ledgers,
        };
        StorageClient::set_admin_rate_limit_config(env, &config);
        // Reset counter so the new config takes effect immediately.
        StorageClient::set_admin_rate_limit_state(
            env,
            &AdminRateLimitState {
                window_start: env.ledger().sequence(),
                count: 0,
            },
        );
        Ok(config)
    }

    /// Assert that `caller` is either the admin or the trusted relay signer.
    ///
    /// Used by status-transition methods which are callable by both roles.
    pub fn assert_is_relay_or_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin = StorageClient::get_admin(env)?;
        let relay = StorageClient::get_relay_signer(env)?;
        if caller != &admin && caller != &relay {
            return Err(ContractError::Unauthorised);
        }
        caller.require_auth();
        Ok(())
    }

    /// Assert that `caller` is specifically the relay signer (not the admin).
    ///
    /// Used by `register_callback` — only the relay may ingest callbacks.
    #[allow(dead_code)]
    pub fn require_relay_signer(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let relay = StorageClient::get_relay_signer(env)?;
        if caller != &relay {
            return Err(ContractError::NotRelaySigner);
        }
        caller.require_auth();
        Ok(())
    }

    /// Assert that `caller` is the configured guardian.
    #[allow(dead_code)]
    pub fn require_guardian(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let guardian = StorageClient::get_guardian(env).ok_or(ContractError::GuardianNotSet)?;
        if caller != &guardian {
            return Err(ContractError::Unauthorised);
        }
        caller.require_auth();
        Ok(())
    }
}
