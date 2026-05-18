//! # Events
//!
//! Every state transition the contract makes is announced via a typed event
//! so that downstream subscribers (Phase 2 Swap Engine, Phase 3 Cross-Chain
//! Bridge, off-chain indexers) can react without polling.
//!
//! ## Event schema convention
//!
//! Each event is emitted as:
//! ```text
//! topics: [Symbol("synapse"), Symbol("<event_name>")]
//! data:   <typed contracttype struct>
//! ```
//!
//! This two-topic convention is consistent with the Stellar Asset Contract
//! standard and makes event filtering straightforward in Horizon / RPC queries.

use soroban_sdk::{contracttype, symbol_short, Env, String};

use crate::types::TransactionStatus;

// ─── Event data structs ───────────────────────────────────────────────────────

/// Emitted by [`SynapseCoreContract::initialize`].
#[contracttype]
pub struct EventInitialised {
    pub admin: soroban_sdk::Address,
    pub relay_signer: soroban_sdk::Address,
    pub ledger: u32,
}

/// Emitted by [`SynapseCoreContract::register_callback`] when a new
/// [`Transaction`] is persisted for the first time.
#[contracttype]
pub struct EventTransactionRegistered {
    pub tx_id: String,
    pub stellar_account: String,
    pub amount: i128,
    pub asset_code: String,
    pub anchor_transaction_id: String,
    pub ledger: u32,
}

/// Emitted on every status change driven by [`SynapseCoreContract::start_processing`],
/// [`SynapseCoreContract::complete_transaction`], or
/// [`SynapseCoreContract::fail_transaction`].
///
/// Phase 2 listens for `new_status == Completed` to trigger the swap flow.
/// Phase 3 listens for `new_status == Completed` after the swap to initiate bridging.
#[contracttype]
pub struct EventStatusChanged {
    pub tx_id: String,
    pub old_status: TransactionStatus,
    pub new_status: TransactionStatus,
    pub ledger: u32,
}

/// Emitted when a transaction reaches terminal state `Completed`.
/// Carries the confirmed Stellar transaction hash for downstream verification.
#[contracttype]
pub struct EventTransactionCompleted {
    pub tx_id: String,
    pub stellar_tx_hash: String,
    pub ledger: u32,
}

/// Emitted when a transaction reaches terminal state `Failed`.
#[contracttype]
pub struct EventTransactionFailed {
    pub tx_id: String,
    pub reason: String,
    pub ledger: u32,
}

/// Emitted when the admin role is transferred.
#[contracttype]
pub struct EventAdminTransferred {
    pub old_admin: soroban_sdk::Address,
    pub new_admin: soroban_sdk::Address,
    pub ledger: u32,
}

// ─── Emitter ─────────────────────────────────────────────────────────────────

pub struct EventEmitter;

impl EventEmitter {
    /// Emit [`EventInitialised`].
    pub fn initialised(
        env: &Env,
        admin: &soroban_sdk::Address,
        relay_signer: &soroban_sdk::Address,
    ) {
        // TODO:
        // env.events().publish(
        //     (symbol_short!("synapse"), symbol_short!("init")),
        //     EventInitialised {
        //         admin: admin.clone(),
        //         relay_signer: relay_signer.clone(),
        //         ledger: env.ledger().sequence(),
        //     },
        // );
        todo!()
    }

    /// Emit [`EventTransactionRegistered`].
    pub fn transaction_registered(env: &Env, tx: &crate::types::Transaction) {
        // TODO:
        // env.events().publish(
        //     (symbol_short!("synapse"), symbol_short!("reg")),
        //     EventTransactionRegistered { ... },
        // );
        todo!()
    }

    /// Emit [`EventStatusChanged`].
    pub fn status_changed(
        env: &Env,
        tx_id: &String,
        old_status: TransactionStatus,
        new_status: TransactionStatus,
    ) {
        // TODO:
        // env.events().publish(
        //     (symbol_short!("synapse"), symbol_short!("status")),
        //     EventStatusChanged {
        //         tx_id: tx_id.clone(),
        //         old_status,
        //         new_status,
        //         ledger: env.ledger().sequence(),
        //     },
        // );
        todo!()
    }

    /// Emit [`EventTransactionCompleted`].
    pub fn transaction_completed(env: &Env, tx_id: &String, stellar_tx_hash: &String) {
        // TODO:
        // env.events().publish(
        //     (symbol_short!("synapse"), symbol_short!("done")),
        //     EventTransactionCompleted { ... },
        // );
        todo!()
    }

    /// Emit [`EventTransactionFailed`].
    pub fn transaction_failed(env: &Env, tx_id: &String, reason: &String) {
        // TODO:
        // env.events().publish(
        //     (symbol_short!("synapse"), symbol_short!("fail")),
        //     EventTransactionFailed { ... },
        // );
        todo!()
    }

    /// Emit [`EventAdminTransferred`].
    pub fn admin_transferred(
        env: &Env,
        old_admin: &soroban_sdk::Address,
        new_admin: &soroban_sdk::Address,
    ) {
        // TODO:
        // env.events().publish(
        //     (symbol_short!("synapse"), symbol_short!("admin")),
        //     EventAdminTransferred { ... },
        // );
        todo!()
    }
}
