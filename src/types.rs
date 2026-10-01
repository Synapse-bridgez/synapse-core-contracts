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

/// Build/commit-hash identifier baked in at compile time.
///
/// CI sets the `SYNAPSE_BUILD_COMMIT` environment variable to the real git
/// commit hash before invoking `cargo build`; the `build.rs` build script
/// forwards it to the compiler via `cargo:rustc-env`, so the value is
/// embedded directly into the deployed WASM. Local dev builds that do not set
/// the variable fall back to `"unknown"` — a genuine CI-built artifact will
/// always carry the real deployed commit hash.
pub const BUILD_COMMIT: &str = env!("SYNAPSE_BUILD_COMMIT");

/// Current on-chain events schema version.
///
/// Mirrors the locked event schema documented in `EVENTS.md`. Surfaced by
/// [`HealthReport`] so dashboard/monitoring consumers can detect a schema
/// drift between the deployed contract and the version they were built
/// against without issuing a separate query.
pub const EVENTS_VERSION: u32 = 1;

// ─── Structured error diagnostics (#167) ──────────────────────────────────────

/// Off-chain handling guidance for a [`ContractError`] variant.
///
/// Consumed by the `synapse-core` relay service's error-handling and alerting
/// logic to decide how to react to a failed call without string-matching.
#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrorHandling {
    /// Transient condition — the caller may safely retry the same call.
    RetrySafe,
    /// Permanent condition — retrying will not help; drop the call.
    NotRetryable,
    /// Requires operator attention; raise an alert.
    AlertWorthy,
    /// Expected/routine rejection; log only, no alert.
    Routine,
}

/// Structured, machine-parseable diagnostic for a [`ContractError`] variant.
///
/// Every `ContractError` variant maps to exactly one `ErrorDiagnostic` via
/// [`ContractError::diagnostic`]. The shape is deliberately fixed — a stable
/// numeric `code` plus a small set of well-typed context fields — so the
/// off-chain relay service can parse it without pattern-matching on free-text
/// strings. See `docs/ERRORS.md` for the full reference table.
#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrorDiagnostic {
    /// Stable numeric error code. Never reused or renumbered once published.
    pub code: u32,
    /// Whether the caller may retry, must drop, or should alert.
    pub handling: ErrorHandling,
    /// Whether this failure warrants an operator alert.
    pub alert: bool,
    /// Whether the failed call may be safely retried as-is.
    pub retry_safe: bool,
}

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

// ─── Bundled deployment metadata (#159) ───────────────────────────────────────

/// Bundled deployment identity returned by
/// [`crate::SynapseCoreContract::contract_metadata`].
///
/// Convenience aggregate of the individual read-only queries so support
/// tooling and dashboards can fetch the full "what exactly is deployed right
/// now" picture in a single round-trip, avoiding inconsistent reads from
/// separate calls made at slightly different times. Each field mirrors the
/// value returned by its corresponding individual query exactly.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractMetadata {
    /// Contract crate version — mirrors `version()`.
    pub version: String,
    /// On-chain storage schema version — mirrors `schema_version()`.
    pub schema_version: u32,
    /// Locked event schema version — mirrors `events_version()`.
    pub events_version: u32,
    /// Git commit hash baked in at compile time — see [`BUILD_COMMIT`].
    pub build_commit: String,
}

// ─── Relay signer set (#161) ──────────────────────────────────────────────────

/// Current relay-signer roster and quorum threshold returned by
/// [`crate::SynapseCoreContract::get_relay_signer_set`].
///
/// Roster/threshold only — deliberately carries no per-signer state (liveness,
/// quarantine, etc.); that belongs to the separate heartbeat query. The
/// `signers` vector is the authoritative current membership and `threshold` is
/// the number of signers that must agree to authorize a relay action.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelaySignerSet {
    /// Current relay-signer roster.
    pub signers: soroban_sdk::Vec<soroban_sdk::Address>,
    /// Number of signers required to authorize a relay action.
    pub threshold: u32,
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

    // ── Wave 2: Collateral Bonding (#143) ────

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

    // ── Timelocked Upgrade (#163) ────────────────────────────────────────────
    /// Singleton: in-flight timelocked upgrade proposal. Absent when no
    /// upgrade is pending. See [`PendingUpgrade`].
    PendingUpgrade,
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

// ─── Timelocked Upgrade (#163) ────────────────────────────────────────────────

/// In-flight timelocked upgrade proposal.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::PendingUpgrade`].
/// Absent when no upgrade is pending. See `SynapseCoreContract::propose_upgrade`
/// and `SynapseCoreContract::execute_upgrade`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PendingUpgrade {
    /// Hash of the new WASM blob to install once the timelock elapses.
    pub wasm_hash: soroban_sdk::BytesN<32>,
    /// Ledger sequence at which the proposal was created.
    pub proposed_at_ledger: u32,
    /// Ledger sequence at which the proposal becomes executable.
    pub executable_at_ledger: u32,
}

// ─── Storage tier report (#168) ───────────────────────────────────────────────

/// Per-tier storage breakdown row, mirroring a single row of the storage-tier
/// table in `COST_MODEL.md`.
///
/// Units match the document exactly: `entries` is a raw count of ledger
/// entries, `bytes` is the serialized byte footprint of those entries, and
/// `rent_stroops` is the projected rent in stroops for the tier's entries.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct StorageTierRow {
    /// Tier name, matching the `COST_MODEL.md` tier labels
    /// (e.g. "instance", "persistent", "temporary").
    pub tier: String,
    /// Number of live ledger entries in this tier.
    pub entries: u32,
    /// Serialized byte footprint of this tier's entries.
    pub bytes: u32,
    /// Projected rent for this tier's entries, in stroops.
    pub rent_stroops: i128,
}

/// `COST_MODEL.md`-aligned storage cost-model report returned by
/// `SynapseCoreContract::get_storage_tier_report()`.
///
/// Structured to mirror the document's existing cost-projection tables
/// directly — same units, same breakdown categories — so operators can compare
/// live on-chain reality against the document's stated projections without any
/// off-chain translation.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CostModelReport {
    /// Ledger sequence at which the report was generated.
    pub generated_at_ledger: u32,
    /// Per-tier breakdown rows, one per storage tier, in `COST_MODEL.md` order.
    pub tiers: soroban_sdk::Vec<StorageTierRow>,
    /// Total live ledger entries across all tiers.
    pub total_entries: u32,
    /// Total serialized byte footprint across all tiers.
    pub total_bytes: u32,
    /// Total projected rent across all tiers, in stroops.
    pub total_rent_stroops: i128,
}
