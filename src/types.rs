//! # Types
//!
//! On-chain equivalents of the `synapse-core` Rust service's domain model.
//! Every struct that touches ledger storage derives [`soroban_sdk::contracttype`].

use soroban_sdk::{contracterror, contracttype, String};

/// Current on-chain storage schema version.
///
/// Bump this whenever [`Transaction`] or [`StorageKey`] layout changes in a
/// way that a running upgrade needs to be aware of. `initialize()` stores it;
/// `SynapseCoreContract::upgrade()` requires the caller to pass the value it
/// currently expects on-chain before proceeding (THREAT_MODEL.md finding
/// F-04). This cannot validate the *new* WASM's schema — Soroban gives the
/// currently-running code no way to introspect an uploaded-but-not-yet-
/// installed WASM blob — so it guards against upgrading the wrong deployment
/// or an unexpected on-chain state, not against an incompatible new binary.
pub const SCHEMA_VERSION: u32 = 1;

/// Current on-chain events schema version.
///
/// Mirrors the locked event schema documented in `EVENTS.md`. Surfaced by
/// [`HealthReport`] so dashboard/monitoring consumers can detect a schema
/// drift between the deployed contract and the version they were built
/// against without issuing a separate query.
pub const EVENTS_VERSION: u32 = 1;

// ─── Transaction status ───────────────────────────────────────────────────────

/// Mirrors the `status` column in the `transactions` table.
///
/// State machine:
/// ```text
/// Pending ──► Processing ──► Completed
///         └──────────────► Failed
/// ```
#[contracttype]
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TransactionStatus {
    /// Initial state — callback received, not yet picked up by the processor.
    Pending,
    /// Off-chain processor has claimed the job; on-chain verification in progress.
    Processing,
    /// Stellar on-chain settlement confirmed; ready for Phase 2 (Swap Engine).
    Completed,
    /// Terminal failure — reason stored in [`Transaction::failure_reason`].
    Failed,
}

// ─── Callback type ────────────────────────────────────────────────────────────

/// Maps the `callback_type` field from the Anchor Platform webhook.
#[contracttype]
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CallbackType {
    Deposit,
    Withdrawal,
}

// ─── Core transaction record ──────────────────────────────────────────────────

/// On-chain mirror of the `transactions` table row.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::Transaction`].
#[contracttype]
#[derive(Clone, Debug)]
pub struct Transaction {
    /// UUID-style unique identifier (generated off-chain, echoed here).
    pub id: String,

    /// Stellar account address of the depositor (G… address, 56 chars).
    pub stellar_account: String,

    /// Deposit amount in stroops (1 XLM = 10_000_000 stroops).
    /// Stored as i128 to match Soroban's native token amount convention.
    pub amount: i128,

    /// Asset code, e.g. "USDC", "USD" (max 12 chars per SEP-11).
    pub asset_code: String,

    /// Asset issuer address. Combined with `asset_code` uniquely identifies
    /// the Stellar asset.
    pub asset_issuer: String,

    /// Current lifecycle status.
    pub status: TransactionStatus,

    /// Ledger sequence number when the transaction was first registered.
    pub created_at_ledger: u32,

    /// Ledger sequence number of the last status update.
    pub updated_at_ledger: u32,

    /// Opaque ID from the Anchor Platform callback payload.
    /// Mirrors `anchor_transaction_id` in the DB schema.
    pub anchor_transaction_id: String,

    /// `deposit` or `withdrawal` — mirrors `callback_type`.
    pub callback_type: CallbackType,

    /// Raw status string received from the Anchor Platform
    /// (e.g. "pending_external", "completed").
    pub callback_status: String,

    /// Stellar transaction hash recorded after on-chain settlement.
    /// Empty string until the transaction reaches `Completed`.
    pub stellar_tx_hash: String,

    /// Short failure reason code — populated only on `Failed`.
    pub failure_reason: String,
}

// ─── Incoming webhook payload ─────────────────────────────────────────────────

/// Payload forwarded by the trusted relay signer when calling
/// [`SynapseCoreContract::register_callback`].
///
/// This is the on-chain equivalent of the `POST /callback/transaction` body
/// handled by the off-chain `synapse-core` service.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CallbackPayload {
    /// Must match an existing or newly-generated transaction UUID.
    pub transaction_id: String,

    /// Stellar account that initiated the deposit.
    pub stellar_account: String,

    /// Deposit amount in stroops.
    pub amount: i128,

    /// Asset code.
    pub asset_code: String,

    /// Asset issuer address.
    pub asset_issuer: String,

    /// Matches `X-Idempotency-Key` header from the Anchor Platform webhook.
    /// Used for deduplication — mirrors Redis-based idempotency off-chain.
    pub idempotency_key: String,

    /// Anchor Platform's internal transaction ID.
    pub anchor_transaction_id: String,

    /// Callback type from the Anchor Platform.
    pub callback_type: CallbackType,

    /// Raw status from the Anchor Platform callback.
    pub callback_status: String,
}

// ─── Storage keys ─────────────────────────────────────────────────────────────

/// Discriminants used as ledger storage keys.
///
/// Persistent storage keys (admin, relay signer, init flag) use `Symbol`-based
/// variants. Per-transaction data is keyed by the transaction ID string.
#[contracttype]
#[derive(Clone, Debug)]
pub enum StorageKey {
    /// Singleton: whether `initialize()` has been called.
    Initialised,
    /// Singleton: current admin address.
    Admin,
    /// Singleton: trusted relay signer address.
    RelaySigner,
    /// Singleton: emergency-pause / circuit-breaker flag.
    ///
    /// When set to `true` the contract refuses new callback ingestion via
    /// [`crate::SynapseCoreContract::register_callback`]. Absent/`false` means
    /// the contract operates normally.
    Paused,
    /// Per-transaction record keyed by transaction ID.
    Transaction(String),
    /// Idempotency key → cached response ledger; keyed by idempotency key.
    IdempotencyKey(String),
    /// Singleton: address nominated to become admin, pending its own
    /// `accept_admin()` call. Absent when no transfer is in progress.
    PendingAdmin,
    /// Singleton: on-chain storage schema version, set at `initialize()`.
    /// See [`SCHEMA_VERSION`].
    SchemaVersion,

    // ── Wave 2: Param Registry (#146) ────────────────────────────────────────
    /// Per-parameter record keyed by param name string.
    Param(String),

    // ── Wave 2: Collateral Bonding (#143) ────────────────────────────────────
    /// Per-signer bond record keyed by the signer address.
    BondRecord(soroban_sdk::Address),
    /// Per-signer unbond request keyed by the signer address.
    /// Absent when no unbond is pending.
    UnbondRequest(soroban_sdk::Address),

    // ── Wave 2: Anchor Rebate (#145) ─────────────────────────────────────────
    /// Per-anchor tier config keyed by the anchor address.
    AnchorTier(soroban_sdk::Address),

    // ── Wave 2: Diagnostic Queries (#151) ────────────────────────────────────
    /// Singleton: current treasury balance in stroops.
    ///
    /// Read directly by [`crate::SynapseCoreContract::get_treasury_balance`]
    /// as a lightweight, dashboard-friendly counterpart to the fuller
    /// `get_fee_reconciliation` report. Absent means zero (no fees accrued
    /// yet), never an error.
    TreasuryBalance,
    /// Singleton: total bonded collateral across all signers, in stroops.
    ///
    /// Read directly by [`crate::SynapseCoreContract::get_bonded_collateral`]
    /// when called with `None`. Absent means zero (no bonding has occurred
    /// yet), never an error.
    TotalBondedCollateral,
}

// ─── Wave 2: Diagnostic Queries (#153) ────────────────────────────────────────

/// Structured, dashboard-friendly diagnostic snapshot returned by
/// [`crate::SynapseCoreContract::health_detailed`].
///
/// This is a purely additive companion to the existing boolean-ish
/// `health()` query: `health()` is left untouched for backward compatibility
/// with current `synapse-web` consumers, while `health_detailed()` composes
/// the same signal together with the per-subsystem state that this Wave
/// added. Every field is derived from a single cheap storage read so the
/// report stays suitable for frequent dashboard polling.
#[contracttype]
#[derive(Clone, Debug)]
pub struct HealthReport {
    /// Whether `initialize()` has been called. `false` means the contract is
    /// not yet configured and every other field should be treated as
    /// provisional.
    pub initialised: bool,

    /// Emergency-pause / circuit-breaker state. `true` means new callback
    /// ingestion via `register_callback` is refused.
    pub paused: bool,

    /// Whether a trusted relay signer has been configured. `false` means
    /// callback ingestion cannot be authorised yet.
    pub relay_signer_set: bool,

    /// Whether an admin transfer is currently in flight (an address has been
    /// nominated via `StorageKey::PendingAdmin` and has not yet accepted).
    pub pending_admin: bool,

    /// Storage-tier footprint summary for the diagnostic-relevant singletons.
    pub storage: StorageFootprint,

    /// On-chain storage schema version, mirroring [`SCHEMA_VERSION`].
    pub schema_version: u32,

    /// On-chain events schema version, mirroring [`EVENTS_VERSION`].
    pub events_version: u32,
}

/// Storage-tier footprint summary embedded in [`HealthReport`].
///
/// Each field is a direct read of an existing singleton key, so assembling
/// the summary adds no meaningful cost to `health_detailed()`. Absent keys
/// are reported as zero rather than as an error.
#[contracttype]
#[derive(Clone, Debug)]
pub struct StorageFootprint {
    /// Current treasury balance in stroops
    /// ([`StorageKey::TreasuryBalance`]). Zero when no fees have accrued.
    pub treasury_balance: i128,

    /// Total bonded collateral across all signers, in stroops
    /// ([`StorageKey::TotalBondedCollateral`]). Zero when no bonding has
    /// occurred.
    pub total_bonded_collateral: i128,
}

// ─── Wave 2: Param Registry (#146) ────────────────────────────────────────────

/// A single on-chain parameter entry.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::Param`].
/// All tunable values (fee rate, unbond delay, slash percentage, fee ceiling,
/// etc.) live here rather than as independent ad-hoc admin-settable fields.
#[contracttype]
#[derive(Clone, Debug

/* … truncated 789 chars — edit only what you need near the top … */
