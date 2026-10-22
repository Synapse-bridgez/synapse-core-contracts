//! # Event definitions and emitters
//!
//! This module defines every event emitted by the contract as a
//! `#[contracttype]` struct, together with the [`EventEmitter`] helper that
//! publishes them to the Soroban host.
//!
//! The structs here are the canonical ground truth for
//! `event_conformance_manifest.toml`: field names, types, and declaration
//! order must match the manifest entries exactly, and every emitter method on
//! [`EventEmitter`] must use the topic symbol recorded in the manifest.

use soroban_sdk::{contracttype, Address, Env, Symbol};

/// Emitted when the contract is initialized.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Initialized {
    /// Address of the account that performed initialization.
    pub admin: Address,
    /// Ledger timestamp at initialization.
    pub timestamp: u64,
}

/// Emitted when the admin is changed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminChanged {
    /// Previous admin address.
    pub old_admin: Address,
    /// New admin address.
    pub new_admin: Address,
    /// Ledger timestamp of the change.
    pub timestamp: u64,
}

/// Emitted when the contract is paused.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Paused {
    /// Address that triggered the pause.
    pub admin: Address,
    /// Ledger timestamp of the pause.
    pub timestamp: u64,
}

/// Emitted when the contract is unpaused.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Unpaused {
    /// Address that triggered the unpause.
    pub admin: Address,
    /// Ledger timestamp of the unpause.
    pub timestamp: u64,
}

/// Emitted when a new account is registered.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountRegistered {
    /// Address of the newly registered account.
    pub account: Address,
    /// Ledger timestamp of registration.
    pub timestamp: u64,
}

/// Emitted when an account is removed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountRemoved {
    /// Address of the removed account.
    pub account: Address,
    /// Ledger timestamp of removal.
    pub timestamp: u64,
}

/// Emitted when a deposit is made.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deposited {
    /// Address making the deposit.
    pub from: Address,
    /// Amount deposited.
    pub amount: i128,
    /// Ledger timestamp of the deposit.
    pub timestamp: u64,
}

/// Emitted when a withdrawal is made.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Withdrawn {
    /// Address receiving the withdrawal.
    pub to: Address,
    /// Amount withdrawn.
    pub amount: i128,
    /// Ledger timestamp of the withdrawal.
    pub timestamp: u64,
}

/// Emitted when a transfer occurs between accounts.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transferred {
    /// Source address.
    pub from: Address,
    /// Destination address.
    pub to: Address,
    /// Amount transferred.
    pub amount: i128,
    /// Ledger timestamp of the transfer.
    pub timestamp: u64,
}

/// Emitted when a fee is collected.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeCollected {
    /// Address from which the fee was collected.
    pub from: Address,
    /// Fee amount collected.
    pub amount: i128,
    /// Ledger timestamp of collection.
    pub timestamp: u64,
}

/// Emitted when a role is granted to an account.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleGranted {
    /// Account receiving the role.
    pub account: Address,
    /// Symbol identifying the role.
    pub role: Symbol,
    /// Address that granted the role.
    pub admin: Address,
    /// Ledger timestamp of the grant.
    pub timestamp: u64,
}

/// Emitted when a role is revoked from an account.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleRevoked {
    /// Account losing the role.
    pub account: Address,
    /// Symbol identifying the role.
    pub role: Symbol,
    /// Address that revoked the role.
    pub admin: Address,
    /// Ledger timestamp of the revocation.
    pub timestamp: u64,
}

/// Emitted when an allowance is approved.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Approved {
    /// Owner granting the allowance.
    pub owner: Address,
    /// Spender receiving the allowance.
    pub spender: Address,
    /// Allowance amount.
    pub amount: i128,
    /// Ledger timestamp of the approval.
    pub timestamp: u64,
}

/// Emitted when an allowance is revoked.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Revoked {
    /// Owner revoking the allowance.
    pub owner: Address,
    /// Spender losing the allowance.
    pub spender: Address,
    /// Ledger timestamp of the revocation.
    pub timestamp: u64,
}

/// Emitted when a configuration parameter is updated.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigUpdated {
    /// Symbol identifying the configuration key.
    pub key: Symbol,
    /// Ledger timestamp of the update.
    pub timestamp: u64,
}

/// Emitted when a batch operation completes.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchCompleted {
    /// Number of items processed in the batch.
    pub count: u32,
    /// Ledger timestamp of completion.
    pub timestamp: u64,
}

/// Emitted when an emergency shutdown is triggered.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmergencyShutdown {
    /// Address that triggered the shutdown.
    pub admin: Address,
    /// Human-readable reason symbol.
    pub reason: Symbol,
    /// Ledger timestamp of the shutdown.
    pub timestamp: u64,
}

/// Emitted when an emergency shutdown is lifted.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmergencyResumed {
    /// Address that lifted the shutdown.
    pub admin: Address,
    /// Ledger timestamp of the resume.
    pub timestamp: u64,
}

/// Emitted when a migration step is performed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Migrated {
    /// Previous schema version.
    pub from_version: u32,
    /// New schema version.
    pub to_version: u32,
    /// Ledger timestamp of the migration.
    pub timestamp: u64,
}

/// Emitted when an upgrade is scheduled.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeScheduled {
    /// New wasm hash being scheduled.
    pub new_wasm_hash: soroban_sdk::BytesN<32>,
    /// Ledger timestamp at which the upgrade becomes effective.
    pub effective_at: u64,
    /// Ledger timestamp of scheduling.
    pub timestamp: u64,
}

/// Emitted when a scheduled upgrade is applied.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeApplied {
    /// Wasm hash that was applied.
    pub new_wasm_hash: soroban_sdk::BytesN<32>,
    /// Ledger timestamp of application.
    pub timestamp: u64,
}

/// Helper for publishing contract events to the Soroban host.
///
/// Each method corresponds to one event struct and publishes it under the
/// topic symbol recorded in `event_conformance_manifest.toml`.
pub struct EventEmitter;

impl EventEmitter {
    /// Publishes an [`Initialized`] event under topic `init`.
    pub fn initialized(env: &Env, event: Initialized) {
        env.events().publish((Symbol::new(env, "init"),), event);
    }

    /// Publishes an [`AdminChanged`] event under topic `admin_chg`.
    pub fn admin_changed(env: &Env, event: AdminChanged) {
        env.events().publish((Symbol::new(env, "admin_chg"),), event);
    }

    /// Publishes a [`Paused`] event under topic `paused`.
    pub fn paused(env: &Env, event: Paused) {
        env.events().publish((Symbol::new(env, "paused"),), event);
    }

    /// Publishes an [`Unpaused`] event under topic `unpaused`.
    pub fn unpaused(env: &Env, event: Unpaused) {
        env.events().publish((Symbol::new(env, "unpaused"),), event);
    }

    /// Publishes an [`AccountRegistered`] event under topic `acct_reg`.
    pub fn account_registered(env: &Env, event: AccountRegistered) {
        env.events().publish((Symbol::new(env, "acct_reg"),), event);
    }

    /// Publishes an [`AccountRemoved`] event under topic `acct_rm`.
    pub fn account_removed(env: &Env, event: AccountRemoved) {
        env.events().publish((Symbol::new(env, "acct_rm"),), event);
    }

    /// Publishes a [`Deposited`] event under topic `deposit`.
    pub fn deposited(env: &Env, event: Deposited) {
        env.events().publish((Symbol::new(env, "deposit"),), event);
    }

    /// Publishes a [`Withdrawn`] event under topic `withdraw`.
    pub fn withdrawn(env: &Env, event: Withdrawn) {
        env.events().publish((Symbol::new(env, "withdraw"),), event);
    }

    /// Publishes a [`Transferred`] event under topic `transfer`.
    pub fn transferred(env: &Env, event: Transferred) {
        env.events().publish((Symbol::new(env, "transfer"),), event);
    }

    /// Publishes a [`FeeCollected`] event under topic `fee`.
    pub fn fee_collected(env: &Env, event: FeeCollected) {
        env.events().publish((Symbol::new(env, "fee"),), event);
    }

    /// Publishes a [`RoleGranted`] event under topic `role_grant`.
    pub fn role_granted(env: &Env, event: RoleGranted) {
        env.events().publish((Symbol::new(env, "role_grant"),), event);
    }

    /// Publishes a [`RoleRevoked`] event under topic `role_revoke`.
    pub fn role_revoked(env: &Env, event: RoleRevoked) {
        env.events().publish((Symbol::new(env, "role_revoke"),), event);
    }

    /// Publishes an [`Approved`] event under topic `approve`.
    pub fn approved(env: &Env, event: Approved) {
        env.events().publish((Symbol::new(env, "approve"),), event);
    }

    /// Publishes a [`Revoked`] event under topic `revoke`.
    pub fn revoked(env: &Env, event: Revoked) {
        env.events().publish((Symbol::new(env, "revoke"),), event);
    }

    /// Publishes a [`ConfigUpdated`] event under topic `cfg_upd`.
    pub fn config_updated(env: &Env, event: ConfigUpdated) {
        env.events().publish((Symbol::new(env, "cfg_upd"),), event);
    }

    /// Publishes a [`BatchCompleted`] event under topic `batch`.
    pub fn batch_completed(env: &Env, event: BatchCompleted) {
        env.events().publish((Symbol::new(env, "batch"),), event);
    }

    /// Publishes an [`EmergencyShutdown`] event under topic `em_shutdown`.
    pub fn emergency_shutdown(env: &Env, event: EmergencyShutdown) {
        env.events().publish((Symbol::new(env, "em_shutdown"),), event);
    }

    /// Publishes an [`EmergencyResumed`] event under topic `em_resume`.
    pub fn emergency_resumed(env: &Env, event: EmergencyResumed) {
        env.events().publish((Symbol::new(env, "em_resume"),), event);
    }

    /// Publishes a [`Migrated`] event under topic `migrate`.
    pub fn migrated(env: &Env, event: Migrated) {
        env.events().publish((Symbol::new(env, "migrate"),), event);
    }

    /// Publishes an [`UpgradeScheduled`] event under topic `upg_sched`.
    pub fn upgrade_scheduled(env: &Env, event: UpgradeScheduled) {
        env.events().publish((Symbol::new(env, "upg_sched"),), event);
    }

    /// Publishes an [`UpgradeApplied`] event under topic `upg_apply`.
    pub fn upgrade_applied(env: &Env, event: UpgradeApplied) {
        env.events().publish((Symbol::new(env, "upg_apply"),), event);
    }
}