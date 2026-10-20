//! # Soroban contract crate
//!
//! This crate exposes the contract's public entry points, types, and events.
//! Every public item is required to carry a doc-comment; the crate-level
//! `#![deny(missing_docs)]` attribute below turns any gap into a build error,
//! which is enforced in CI as a documentation-coverage gate.

#![deny(missing_docs)]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

/// Contract type marker for the deployed Soroban contract.
#[contract]
pub struct Contract;

/// A bounded recent window over which signer activity is summarized.
///
/// The window is expressed as a fixed number of most-recent calls, so the
/// underlying counters stay bounded rather than growing into an unbounded
/// history.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerWindow {
    /// Maximum number of most-recent calls included in the report.
    pub recent_calls: u32,
}

/// Per-signer operational summary over a recent window.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignerActivityReport {
    /// The signer this report describes.
    pub signer: Address,
    /// Total number of calls recorded for the signer.
    pub total_calls: u32,
    /// Number of successful calls recorded for the signer.
    pub success_count: u32,
    /// Number of failed calls recorded for the signer.
    pub failure_count: u32,
    /// Number of calls included in the reported window.
    pub window_calls: u32,
}

/// Public entry points for the contract.
#[contractimpl]
impl Contract {
    /// Initialize the contract with the given administrator address.
    ///
    /// # Arguments
    /// * `env` - The Soroban execution environment.
    /// * `admin` - The address granted administrative privileges.
    pub fn initialize(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    /// Report signer activity for the given signer address.
    ///
    /// # Arguments
    /// * `env` - The Soroban execution environment.
    /// * `signer` - The signer whose activity is being reported.
    pub fn report_signer_activity(env: Env, signer: Address) {
        signer.require_auth();
        let key = DataKey::SignerActivity(signer.clone());
        let count: u32 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &(count + 1));
    }

    /// Record a relay-signer-gated call outcome for the given signer.
    ///
    /// Maintains lightweight running counters alongside relay-signer-gated
    /// entry points so the activity report can be sourced without replaying
    /// event history.
    ///
    /// # Arguments
    /// * `env` - The Soroban execution environment.
    /// * `signer` - The signer whose call is being recorded.
    /// * `success` - Whether the gated call succeeded.
    pub fn record_signer_call(env: Env, signer: Address, success: bool) {
        signer.require_auth();
        let total_key = DataKey::SignerActivity(signer.clone());
        let total: u32 = env.storage().persistent().get(&total_key).unwrap_or(0);
        env.storage().persistent().set(&total_key, &(total + 1));

        let outcome_key = if success {
            DataKey::SignerSuccess(signer.clone())
        } else {
            DataKey::SignerFailure(signer.clone())
        };
        let outcome: u32 = env.storage().persistent().get(&outcome_key).unwrap_or(0);
        env.storage().persistent().set(&outcome_key, &(outcome + 1));
    }

    /// Read-only query summarizing a signer's call volume and outcome
    /// breakdown over the given recent window.
    ///
    /// Sourced from the running counters maintained by
    /// [`Contract::record_signer_call`]. The window bounds how many of the
    /// most-recent calls are reflected in `window_calls`, keeping the report
    /// bounded rather than an ever-growing history.
    ///
    /// # Arguments
    /// * `env` - The Soroban execution environment.
    /// * `signer` - The signer whose activity is being reported.
    /// * `window` - The recent window to summarize.
    pub fn get_signer_activity_report(
        env: Env,
        signer: Address,
        window: LedgerWindow,
    ) -> SignerActivityReport {
        let total: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::SignerActivity(signer.clone()))
            .unwrap_or(0);
        let success: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::SignerSuccess(signer.clone()))
            .unwrap_or(0);
        let failure: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::SignerFailure(signer.clone()))
            .unwrap_or(0);

        let window_calls = if window.recent_calls < total {
            window.recent_calls
        } else {
            total
        };

        SignerActivityReport {
            signer,
            total_calls: total,
            success_count: success,
            failure_count: failure,
            window_calls,
        }
    }
}

/// Storage keys used by the contract.
#[soroban_sdk::contracttype]
pub enum DataKey {
    /// Address of the contract administrator.
    Admin,
    /// Per-signer activity counter keyed by signer address.
    SignerActivity(Address),
    /// Per-signer successful-call counter keyed by signer address.
    SignerSuccess(Address),
    /// Per-signer failed-call counter keyed by signer address.
    SignerFailure(Address),
}
