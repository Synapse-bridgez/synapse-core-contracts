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

use soroban_sdk::{contract, contractimpl, Address, BytesN, Env};

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

    /// Propose a new admin. Only the current admin may call this.
    ///
    /// Overwrites any previously pending proposal rather than stacking.
    pub fn propose_admin(env: Env, new_admin: Address) -> Result<(), AdminError> {
        let current = storage::get_admin(&env).ok_or(AdminError::NotInitialized)?;
        current.require_auth();
        storage::set_pending_admin(&env, &new_admin);
        Ok(())
    }

    /// Accept a pending admin transfer. Only the stored `pending_admin` may call this.
    ///
    /// Finalizes the transfer, clears `pending_admin`, and emits the `admin` event.
    pub fn accept_admin(env: Env, caller: Address) -> Result<(), AdminError> {
        caller.require_auth();
        let pending = storage::get_pending_admin(&env).ok_or(AdminError::NoPendingAdmin)?;
        if caller != pending {
            return Err(AdminError::NotPendingAdmin);
        }
        storage::set_admin(&env, &pending);
        storage::clear_pending_admin(&env);
        events::admin_transferred(&env, &pending);
        Ok(())
    }

    /// Read-only query returning the current admin, if set.
    pub fn admin(env: Env) -> Option<Address> {
        storage::get_admin(&env)
    }

    /// Read-only query returning the pending admin proposal, if any.
    pub fn pending_admin(env: Env) -> Option<Address> {
        storage::get_pending_admin(&env)
    }

    /// Rotate the single relay signer immediately (no timelock).
    ///
    /// Admin-gated via the shared `require_admin` helper. Persists `new_signer`
    /// so `relay_signer()` returns it right away and `register_callback`
    /// authenticates against the updated value. Emits `EventRelaySignerRotated`
    /// carrying the old and new signer addresses on every successful call,
    /// including no-op rotations to the currently-active signer.
    pub fn set_relay_signer(env: Env, new_signer: Address) -> Result<(), AdminError> {
        admin::require_admin(&env)?;
        let old_signer = storage::get_relay_signer(&env);
        storage::set_relay_signer(&env, &new_signer);
        events::relay_signer_rotated(&env, &old_signer, &new_signer);
        Ok(())
    }

    /// Returns the live schema version of the contract's storage layout.
    ///
    /// This is a cheap query backed by [`SCHEMA_VERSION`]; it is not derived
    /// from WASM metadata.
    pub fn schema_version(_env: Env) -> u32 {
        SCHEMA_VERSION
    }

    /// Return the current contract semver.
    ///
    /// Stable across calls within a single deployment and independent from
    /// `schema_version()`. Bumped on schema changes per `EVENTS.md`.
    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    /// Minimal liveness / self-consistency check.
    ///
    /// Returns `true` only when both the admin and relay signer are set. Safe to
    /// call before `initialize()`: it reports `false` rather than panicking.
    pub fn health(env: Env) -> bool {
        storage::get_admin(&env).is_some() && storage::get_relay_signer(&env).is_some()
    }

    /// Upgrades the contract WASM to `new_wasm_hash`.
    ///
    /// Admin-gated via `require_auth()`. The caller's `expected_schema_version`
    /// must match the live [`schema_version`](Self::schema_version); otherwise
    /// the call is rejected with [`ContractError::SchemaVersionMismatch`]
    /// *before* `env.deployer().update_current_contract_wasm()` is invoked.
    ///
    /// On success, `EventContractUpgraded` ("upgrade" topic) is emitted.
    /// Persistent and instance storage survive the upgrade; only temporary
    /// storage (idempotency keys) may be evicted.
    pub fn upgrade(
        env: Env,
        new_wasm_hash: BytesN<32>,
        expected_schema_version: u32,
    ) -> Result<(), ContractError> {
        // Cheapest-first ordering: authenticate the admin before any other work.
        let admin: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Admin)
            .ok_or(ContractError::Unauthorized)?;
        admin.require_auth();

        // Schema guard: reject mismatches before touching WASM.
        if expected_schema_version != SCHEMA_VERSION {
            return Err(ContractError::SchemaVersionMismatch);
        }

        // Only now is it safe to swap the contract code.
        env.deployer()
            .update_current_contract_wasm(new_wasm_hash.clone());

        env.events().publish(
            (UPGRADE_TOPIC,),
            EventContractUpgraded {
                new_wasm_hash,
                schema_version: SCHEMA_VERSION,
            },
        );

        Ok(())
    }

    /// Emergency admin revocation, callable only by the guardian.
    ///
    /// Per `docs/adr/0004-guardian-emergency-admin-revocation.md`, the guardian
    /// may revoke a compromised admin and replace it with `new_admin`. This
    /// entry point is gated so it can *only* revoke/replace the admin — it
    /// cannot grant the guardian any other admin powers. The guardian's
    /// `require_auth()` is enforced before any storage mutation.
    ///
    /// On success the stored admin is replaced immediately (so every
    /// admin-gated entry point, e.g. [`upgrade`](Self::upgrade), respects the
    /// new admin on the very next call) and `EventGuardianAdminRevoked` is
    /// emitted under the distinct `guardian_revoke` topic.
    pub fn guardian_revoke_admin(
        env: Env,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        // Authenticate the guardian before any other work.
        let guardian: Address = env
            .storage()
            .instance()
            .get(&StorageKey::Guardian)
            .ok_or(ContractError::Unauthorized)?;
        guardian.require_auth();

        // Capture the outgoing admin for the event (may be absent if already
        // revoked); the guardian still may install a replacement.
        let old_admin: Option<Address> = env.storage().instance().get(&StorageKey::Admin);

        // Revoke/replace only: overwrite the admin slot. No other admin powers
        // are granted to the guardian by this entry point.
        env.storage()
            .instance()
            .set(&StorageKey::Admin, &new_admin);

        env.events().publish(
            (GUARDIAN_REVOKE_TOPIC,),
            EventGuardianAdminRevoked {
                old_admin,
                new_admin,
            },
        );

        Ok(())
    }
}

// Re-export the admin error type for callers.
pub use admin::AdminError as ContractError;

/// Compile-time guard that `ContractError` stays within Soroban's 50-variant cap.
///
/// `ContractError::__VARIANT_COUNT` is a `const` emitted alongside the enum in
/// `src/types.rs`; if a future PR adds a variant past the ceiling this module
/// fails to compile, so the cap cannot silently regress.
const _: () = assert!(
    crate::types::ContractError::__VARIANT_COUNT <= MAX_CONTRACT_ERROR_VARIANTS,
    "ContractError exceeds Soroban's 50-variant cap; see DECISIONS.md 'Error-code budget'"
);
