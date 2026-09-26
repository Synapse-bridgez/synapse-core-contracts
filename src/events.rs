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
//!
//! **Public API:** topic names, payload fields/types/order, and multi-event
//! emission order are versioned for Phase 2 / Phase 3 subscribers. See
//! [`EVENTS.md`](../EVENTS.md) (catalogue + semver) and
//! [`CHANGELOG.md`](../CHANGELOG.md#event-schema).
//!
//! ## Additive trailing fields (semver policy)
//!
//! To add a field to an existing event without a major-version bump, append it
//! as a trailing `Option<T>` field. Soroban encodes `#[contracttype]` structs as
//! a positional `ScVal::Map` keyed by field name, so a subscriber decoding with
//! the *old* schema simply ignores the extra key, while a subscriber decoding
//! with the *new* schema reads `None` for payloads emitted before the field
//! existed. This is the required convention for all future additive changes —
//! see [`EVENTS.md`](../EVENTS.md#additive-trailing-fields).

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
///
/// `correlation_id` is an additive trailing field (see module docs): it is
/// `None` for payloads emitted before the field existed, and old subscribers
/// decoding the pre-additive schema ignore the extra key entirely.
#[contracttype]
pub struct EventStatusChanged {
    pub tx_id: String,
    pub old_status: TransactionStatus,
    pub new_status: TransactionStatus,
    pub ledger: u32,
    /// Additive trailing field — optional correlation identifier for
    /// cross-service tracing. `None` when not supplied by the caller.
    pub correlation_id: Option<String>,
}

/// Emitted when a transaction reaches terminal state `Completed`.
/// Carries the confirmed Stellar transaction hash for downstream verification.
///
/// `correlation_id` is an additive trailing field (see module docs).
#[contracttype]
pub struct EventTransactionCompleted {
    pub tx_id: String,
    pub stellar_tx_hash: String,
    pub ledger: u32,
    /// Additive trailing field — optional correlation identifier for
    /// cross-service tracing. `None` when not supplied by the caller.
    pub correlation_id: Option<String>,
}

/// Emitted when a transaction reaches terminal state `Failed`.
#[contracttype]
pub struct EventTransactionFailed {
    pub tx_id: String,
    pub reason: String,
    pub ledger: u32,
}

/// Emitted by [`SynapseCoreContract::expire_transaction`] when a stale
/// `Pending` transaction is auto-expired by the state machine.
///
/// Emitted exactly once per successful `expire_transaction` call, and never
/// on a rejected/failed attempt. Subscribers should treat this as a terminal
/// transition equivalent to [`EventTransactionFailed`].
#[contracttype]
pub struct EventTransactionExpired {
    /// Identifier of the transaction that was expired.
    pub tx_id: String,
    /// Ledger sequence at which the expiry was recorded.
    pub expired_at: u32,
}

/// Emitted when the admin role is transferred.
#[contracttype]
pub struct EventAdminTransferred {
    pub old_admin: soroban_sdk::Address,
    pub new_admin: soroban_sdk::Address,
    pub ledger: u32,
}

/// Emitted by [`SynapseCoreContract::propose_admin`] when a new admin is
/// nominated. The transfer is not yet effective at this point — see
/// [`EventAdminTransferred`], emitted only once the nominee itself calls
/// `accept_admin` (two-step transfer, THREAT_MODEL.md finding F-03).
#[contracttype]
pub struct EventAdminTransferProposed {
    pub current_admin: soroban_sdk::Address,
    pub proposed_admin: soroban_sdk::Address,
    pub ledger: u32,
}

/// Emitted when the trusted relay signer is rotated.
///
/// Relay-signer compromise lets an attacker register forged callbacks and
/// drive the lifecycle state machine, so off-chain monitoring subscribes to
/// this the same way it does [`EventAdminTransferred`].
#[contracttype]
pub struct EventRelaySignerRotated {
    pub old_signer: soroban_sdk::Address,
    pub new_signer: soroban_sdk::Address,
    pub ledger: u32,
}

/// Emitted by [`SynapseCoreContract::upgrade`] when the contract WASM is
/// replaced in-place.
///
/// Downstream indexers and monitoring tooling subscribe to this to detect
/// unexpected upgrades (potential admin-key compromise).
#[contracttype]
pub struct EventContractUpgraded {
    /// Admin that authorised the upgrade.
    pub admin: soroban_sdk::Address,
    /// The new WASM hash (SHA-256 of the deployed `.wasm`).
    pub new_wasm_hash: soroban_sdk::BytesN<32>,
    pub ledger: u32,
    /// The on-chain schema version that `expected_schema_version` was
    /// checked against before this upgrade proceeded (THREAT_MODEL.md
    /// finding F-04). Additive trailing field — see EVENTS.md semver policy.
    pub schema_version: u32,
}

/// Emitted by [`SynapseCoreContract::pause`] / [`SynapseCoreContract::unpause`]
/// whenever the emergency circuit breaker is toggled.
///
/// Off-chain incident-response tooling subscribes to this so a pause is
/// observable on-chain the moment it happens.
#[contracttype]
pub struct EventPauseToggled {
    /// New pause state: `true` = paused, `false` = unpaused.
    pub paused: bool,
    /// Admin address that performed the toggle.
    pub admin: soroban_sdk::Address,
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
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("init")),
            EventInitialised {
                admin: admin.clone(),
                relay_signer: relay_signer.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventTransactionRegistered`].
    pub fn transaction_registered(env: &Env, tx: &crate::types::Transaction) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("reg")),
            EventTransactionRegistered {
                tx_id: tx.id.clone(),
                stellar_account: tx.stellar_account.clone(),
                amount: tx.amount,
                asset_code: tx.asset_code.clone(),
                anchor_transaction_id: tx.anchor_transaction_id.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventTransactionExpired`].
    ///
    /// Called exactly once per successful `expire_transaction` invocation,
    /// after the transaction has been moved to its terminal expired state.
    pub fn transaction_expired(env: &Env, tx_id: &String) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("expire")),
            EventTransactionExpired {
                tx_id: tx_id.clone(),
                expired_at: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventContractUpgraded`].
    pub fn contract_upgraded(
        env: &Env,
        admin: &soroban_sdk::Address,
        new_wasm_hash: &soroban_sdk::BytesN<32>,
        schema_version: u32,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("upgrade")),
            EventContractUpgraded {
                admin: admin.clone(),
                new_wasm_hash: new_wasm_hash.clone(),
                ledger: env.ledger().sequence(),
                schema_version,
            },
        );
    }

    /// Emit [`EventAdminTransferProposed`].
    pub fn admin_transfer_proposed(
        env: &Env,
        current_admin: &soroban_sdk::Address,
        proposed_admin: &soroban_sdk::Address,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("admin_prop")),
            EventAdminTransferProposed {
                current_admin: current_admin.clone(),
                proposed_admin: proposed_admin.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventStatusChanged`].
    ///
    /// `correlation_id` is an additive trailing field: pass `None` to preserve
    /// the pre-additive payload shape for existing subscribers.
    pub fn status_changed(
        env: &Env,
        tx_id: &String,
        old_status: &TransactionStatus,
        new_status: &TransactionStatus,
        correlation_id: Option<String>,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("status")),
            EventStatusChanged {
                tx_id: tx_id.clone(),
                old_status: old_status.clone(),
                new_status: new_status.clone(),
                ledger: env.ledger().sequence(),
                correlation_id,
            },
        );
    }

    /// Emit [`EventTransactionCompleted`].
    ///
    /// `correlation_id` is an additive trailing field: pass `None` to preserve
    /// the pre-additive payload shape for existing subscribers.
    pub fn transaction_completed(
        env: &Env,
        tx_id: &String,
        stellar_tx_hash: &String,
        correlation_id: Option<String>,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("done")),
            EventTransactionCompleted {
                tx_id: tx_id.clone(),
                stellar_tx_hash: stellar_tx_hash.clone(),
                ledger: env.ledger().sequence(),
                correlation_id,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Events, Env, IntoVal, String, TryFromVal, Val};

    /// Old-shape decoder: mirrors a subscriber compiled against the
    /// pre-additive `EventStatusChanged` schema (no `correlation_id`).
    #[contracttype]
    struct OldEventStatusChanged {
        pub tx_id: String,
        pub old_status: TransactionStatus,
        pub new_status: TransactionStatus,
        pub ledger: u32,
    }

    /// Old-shape decoder for the pre-additive `EventTransactionCompleted`.
    #[contracttype]
    struct OldEventTransactionCompleted {
        pub tx_id: String,
        pub stellar_tx_hash: String,
        pub ledger: u32,
    }

    #[test]
    fn old_decoder_handles_new_status_payload() {
        let env = Env::default();
        let tx_id = String::from_str(&env, "tx-1");

        EventEmitter::status_changed(
            &env,
            &tx_id,
            &TransactionStatus::Pending,
            &TransactionStatus::Processing,
            Some(String::from_str(&env, "corr-1")),
        );

        let events = env.events().all();
        let (_, _, data): (Val, Val, Val) = events.last().unwrap();

        // Old-shape decoding must succeed and ignore the additive field.
        let decoded = OldEventStatusChanged::try_from_val(&env, &data)
            .expect("old decoder must tolerate additive trailing field");
        assert_eq!(decoded.tx_id, tx_id);
        assert_eq!(decoded.new_status, TransactionStatus::Processing);
    }

    #[test]
    fn old_decoder_handles_new_completed_payload() {
        let env = Env::default();
        let tx_id = String::from_str(&env, "tx-2");
        let hash = String::from_str(&env, "hash-2");

        EventEmitter::transaction_completed(&env, &tx_id, &hash, None);

        let events = env.events().all();
        let (_, _, data): (Val, Val, Val) = events.last().unwrap();

        let decoded = OldEventTransactionCompleted::try_from_val(&env, &data)
            .expect("old decoder must tolerate additive trailing field");
        assert_eq!(decoded.tx_id, tx_id);
        assert_eq!(decoded.stellar_tx_hash, hash);
    }

    #[test]
    fn new_decoder_reads_none_for_absent_additive_field() {
        let env = Env::default();
        let tx_id = String::from_str(&env, "tx-3");

        EventEmitter::status_changed(
            &env,
            &tx_id,
            &TransactionStatus::Pending,
            &TransactionStatus::Processing,
            None,
        );

        let events = env.events().all();
        let (_, _, data): (Val, Val, Val) = events.last().unwrap();

        let decoded = EventStatusChanged::try_from_val(&env, &data)
            .expect("new decoder must read additive field");
        assert_eq!(decoded.correlation_id, None);
    }
}
