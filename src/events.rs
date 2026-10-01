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

/// Emitted exactly once by [`SynapseCoreContract::batch_register_callback`],
/// after every per-item [`EventTransactionRegistered`] of that batch.
#[contracttype]
pub struct EventBatchProcessed {
    pub caller: soroban_sdk::Address,
    pub batch_size: u32,
    pub first_tx_id: String,
    pub last_tx_id: String,
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

/// Emitted by [`SynapseCoreContract::propose_admin`] when a new admin is
/// nominated. The transfer is not yet effective at this point — see
/// [`EventAdminTransferred`], emitted only once the nominee itself calls
/// `accept_admin` (two-step transfer, `THREAT_MODEL.md` finding F-03).
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
    /// checked against before this upgrade proceeded (`THREAT_MODEL.md`
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

// ─── Wave 2 event data structs ────────────────────────────────────────────────

// ── Param Registry (#146) ──────────────────────────────────────────────────────

/// Emitted by [`SynapseCoreContract::set_param`] when a parameter value is set
/// or updated by the admin.
#[contracttype]
pub struct EventParamSet {
    /// Name of the parameter.
    pub name: String,
    /// New value.
    pub value: i128,
    /// Admin that set the value.
    pub admin: soroban_sdk::Address,
    pub ledger: u32,
}

// ── Collateral Bonding (#143) ──────────────────────────────────────────────────

/// Emitted by [`SynapseCoreContract::bond_collateral`] when a relay signer
/// increases their bonded collateral.
#[contracttype]
pub struct EventBonded {
    /// The relay signer address that bonded.
    pub signer: soroban_sdk::Address,
    /// Amount added in this call (not the total).
    pub amount: i128,
    /// New total bonded amount after this call.
    pub total: i128,
    pub ledger: u32,
}

/// Emitted by [`SynapseCoreContract::unbond_collateral`] when a relay signer
/// requests an unbond (starts the unbonding delay).
#[contracttype]
pub struct EventUnbondRequested {
    pub signer: soroban_sdk::Address,
    /// Amount requested to unbond.
    pub amount: i128,
    /// Ledger at which `claim_unbond` will become callable.
    pub claimable_at_ledger: u32,
    pub ledger: u32,
}

/// Emitted by [`SynapseCoreContract::claim_unbond`] when a relay signer
/// successfully claims their unbonded collateral after the delay elapses.
#[contracttype]
pub struct EventUnbondClaimed {
    pub signer: soroban_sdk::Address,
    /// Amount claimed/released.
    pub amount: i128,
    pub ledger: u32,
}

// ── Slashing (#144) ────────────────────────────────────────────────────────────

/// Emitted by [`SynapseCoreContract::slash_signer`] when a relay signer is
/// slashed for on-chain-provable misbehaviour.
#[contracttype]
pub struct EventSlashed {
    /// The signer that was slashed.
    pub signer: soroban_sdk::Address,
    /// Amount slashed (burned / redirected).
    pub slashed_amount: i128,
    /// Remaining bonded amount after slashing.
    pub remaining_bond: i128,
    /// The `transaction_id` from the conflicting-callback evidence.
    pub evidence_tx_id: String,
    /// Admin (or guardian) that triggered the slash.
    pub caller: soroban_sdk::Address,
    pub ledger: u32,
}

// ── Disputes (#112) ────────────────────────────────────────────────────────────

/// Emitted when a dispute is raised against a transaction.
///
/// A dispute may be raised more than once over a transaction's lifetime if
/// the dispute state machine permits re-disputing after a prior resolution.
/// Subscribers correlate raised/resolved pairs via the shared `tx_id` field.
///
/// Downstream support tooling and admin dashboards subscribe to this event
/// to surface disputes in real time without polling ledger state.
///
/// Subscribers correlate this with the following [`EventDisputeResolved`] for
/// the same `tx_id` to reconstruct the full dispute lifecycle.
#[contracttype]
pub struct EventDisputeRaised {
    /// The transaction ID under dispute — shared with [`EventDisputeResolved`]
    /// as the correlation key.
    pub tx_id: String,
    /// Short human-readable reason code supplied by the caller
    /// (e.g. `"amount_mismatch"`, `"missing_settlement"`).
    pub reason: String,
    /// Address that raised the dispute (relay signer or admin).
    pub caller: soroban_sdk::Address,
    pub ledger: u32,
}

/// Emitted when a raised dispute is resolved.
///
/// Subscribers correlate this with the preceding [`EventDisputeRaised`] for
/// the same `tx_id` to reconstruct the full dispute lifecycle.
///
/// # `upheld` semantics
///
/// * `upheld = true`  — The dispute is **upheld**: the transaction is
///   considered invalid and the outcome is **reverted to `Failed`**.
///   Downstream systems (support tooling, audit logs) should treat this as
///   a terminal failure equivalent to [`EventTransactionFailed`].
///
/// * `upheld = false` — The dispute is **rejected**: the original outcome
///   stands and the transaction is **returned to `Completed`**.
///   Downstream systems should resume treating the transaction as settled.
#[contracttype]
pub struct EventDisputeResolved {
    /// The transaction ID — shared correlation key with [`EventDisputeRaised`].
    pub tx_id: String,
    /// Whether the dispute was upheld (`true` → reverted to `Failed`) or
    /// rejected (`false` → returned to `Completed`). See doc-comment above
    /// for the full semantics.
    pub upheld: bool,
    /// Address that resolved the dispute (admin only).
    pub caller: soroban_sdk::Address,
    pub ledger: u32,
}

// ── Anchor Rebate (#145) ────────────────────────────────────────────────────────

/// Emitted by [`SynapseCoreContract::set_anchor_tier`] when the admin sets or
/// updates an anchor's rebate tier.
#[contracttype]
pub struct EventAnchorTierSet {
    pub anchor: soroban_sdk::Address,
    /// Rebate in basis points (`0–10_000`).
    pub rebate_bps: u32,
    /// Human-readable tier label.
    pub label: String,
    pub admin: soroban_sdk::Address,
    pub ledger: u32,
}

/// Emitted by [`SynapseCoreContract::compute_effective_fee`] when the rebate
/// is applied to a base fee amount (informational / audit trail).
#[contracttype]
pub struct EventRebateApplied {
    pub anchor: soroban_sdk::Address,
    /// Base fee before rebate (in the same unit as the fee param).
    pub base_fee: i128,
    /// Effective fee after applying the rebate.
    pub effective_fee: i128,
    /// Rebate in basis points that was applied.
    pub rebate_bps: u32,
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

    /// Emit [`EventBatchProcessed`].
    pub fn batch_processed(
        env: &Env,
        caller: &soroban_sdk::Address,
        batch_size: u32,
        first_tx_id: &String,
        last_tx_id: &String,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("batch")),
            EventBatchProcessed {
                caller: caller.clone(),
                batch_size,
                first_tx_id: first_tx_id.clone(),
                last_tx_id: last_tx_id.clone(),
                ledger: env.ledger().sequence(),
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
            (symbol_short!("synapse"), symbol_short!("propose")),
            EventAdminTransferProposed {
                current_admin: current_admin.clone(),
                proposed_admin: proposed_admin.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventRelaySignerRotated`].
    pub fn relay_signer_rotated(
        env: &Env,
        old_signer: &soroban_sdk::Address,
        new_signer: &soroban_sdk::Address,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("relay")),
            EventRelaySignerRotated {
                old_signer: old_signer.clone(),
                new_signer: new_signer.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventPauseToggled`].
    pub fn pause_toggled(env: &Env, paused: bool, admin: &soroban_sdk::Address) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("pause")),
            EventPauseToggled {
                paused,
                admin: admin.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventStatusChanged`].
    pub fn status_changed(
        env: &Env,
        tx_id: &String,
        old_status: TransactionStatus,
        new_status: TransactionStatus,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("status")),
            EventStatusChanged {
                tx_id: tx_id.clone(),
                old_status,
                new_status,
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventTransactionCompleted`].
    pub fn transaction_completed(env: &Env, tx_id: &String, stellar_tx_hash: &String) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("done")),
            EventTransactionCompleted {
                tx_id: tx_id.clone(),
                stellar_tx_hash: stellar_tx_hash.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventTransactionFailed`].
    pub fn transaction_failed(env: &Env, tx_id: &String, reason: &String) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("fail")),
            EventTransactionFailed {
                tx_id: tx_id.clone(),
                reason: reason.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventAdminTransferred`].
    pub fn admin_transferred(
        env: &Env,
        old_admin: &soroban_sdk::Address,
        new_admin: &soroban_sdk::Address,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("admin")),
            EventAdminTransferred {
                old_admin: old_admin.clone(),
                new_admin: new_admin.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    // ── Wave 2 emitters ───────────────────────────────────────────────────────

    /// Emit [`EventParamSet`] — param registry update (#146).
    pub fn param_set(env: &Env, name: &String, value: i128, admin: &soroban_sdk::Address) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("param")),
            EventParamSet {
                name: name.clone(),
                value,
                admin: admin.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventBonded`] — collateral bonded (#143).
    pub fn bonded(env: &Env, signer: &soroban_sdk::Address, amount: i128, total: i128) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("bonded")),
            EventBonded {
                signer: signer.clone(),
                amount,
                total,
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventUnbondRequested`] — unbond initiated (#143).
    pub fn unbond_requested(
        env: &Env,
        signer: &soroban_sdk::Address,
        amount: i128,
        claimable_at_ledger: u32,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("unbondrq")),
            EventUnbondRequested {
                signer: signer.clone(),
                amount,
                claimable_at_ledger,
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventUnbondClaimed`] — unbond claimed (#143).
    pub fn unbond_claimed(env: &Env, signer: &soroban_sdk::Address, amount: i128) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("unbondcl")),
            EventUnbondClaimed {
                signer: signer.clone(),
                amount,
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventSlashed`] — signer slashed (#144).
    pub fn slashed(
        env: &Env,
        signer: &soroban_sdk::Address,
        slashed_amount: i128,
        remaining_bond: i128,
        evidence_tx_id: &String,
        caller: &soroban_sdk::Address,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("slashed")),
            EventSlashed {
                signer: signer.clone(),
                slashed_amount,
                remaining_bond,
                evidence_tx_id: evidence_tx_id.clone(),
                caller: caller.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventDisputeRaised`].
    ///
    /// Topics: `synapse` / `dispute`.
    pub fn dispute_raised(
        env: &Env,
        tx_id: &String,
        reason: &String,
        caller: &soroban_sdk::Address,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("dispute")),
            EventDisputeRaised {
                tx_id: tx_id.clone(),
                reason: reason.clone(),
                caller: caller.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventAnchorTierSet`] — anchor tier updated (#145).
    pub fn anchor_tier_set(
        env: &Env,
        anchor: &soroban_sdk::Address,
        rebate_bps: u32,
        label: &String,
        admin: &soroban_sdk::Address,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("tierset")),
            EventAnchorTierSet {
                anchor: anchor.clone(),
                rebate_bps,
                label: label.clone(),
                admin: admin.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventRebateApplied`] — rebate computation (#145).
    pub fn rebate_applied(
        env: &Env,
        anchor: &soroban_sdk::Address,
        base_fee: i128,
        effective_fee: i128,
        rebate_bps: u32,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("rebate")),
            EventRebateApplied {
                anchor: anchor.clone(),
                base_fee,
                effective_fee,
                rebate_bps,
                ledger: env.ledger().sequence(),
            },
        );
    }

    /// Emit [`EventDisputeResolved`].
    ///
    /// Topics: `synapse` / `dsprslvd`.
    ///
    /// `upheld = true`  → dispute upheld, transaction reverted to `Failed`.
    /// `upheld = false` → dispute rejected, transaction returned to `Completed`.
    pub fn dispute_resolved(
        env: &Env,
        tx_id: &String,
        upheld: bool,
        caller: &soroban_sdk::Address,
    ) {
        env.events().publish(
            (symbol_short!("synapse"), symbol_short!("dsprslvd")),
            EventDisputeResolved {
                tx_id: tx_id.clone(),
                upheld,
                caller: caller.clone(),
                ledger: env.ledger().sequence(),
            },
        );
    }
}
