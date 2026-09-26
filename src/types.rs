//! # Types
//!
//! On-chain equivalents of the `synapse-core` Rust service's domain model.
//! Every struct that touches ledger storage derives [`soroban_sdk::contracttype`].

use soroban_sdk::{contracterror, contracttype, Address, BytesN, String, Vec};

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
///
/// Version history:
/// - `1` — original un-namespaced [`LegacyStorageKey`] layout
/// - `2` — namespaced [`StorageKey`] / [`DataKey`] layout (#89)
pub const SCHEMA_VERSION: u32 = 2;

/// Namespace discriminant baked into every [`StorageKey`] (#89).
///
/// All ledger keys are stored as [`StorageKey::Ns`]`(STORAGE_KEY_NAMESPACE, DataKey)`.
/// Additive schema changes (new [`DataKey`] variants, new fields on existing
/// structs) stay inside the current namespace. A schema bump that relocates
/// keys uses a new namespace value and an `upgrade_and_migrate` /
/// [`crate::SynapseCoreContract::migrate_storage_keys`] path so old and new
/// key-spaces never collide.
///
/// **Convention for future additions:** every new persistent / temporary /
/// instance key MUST be a [`DataKey`] variant constructed via
/// [`StorageKey::ns`]. Do not introduce bare unit-variant keys alongside
/// this enum — the exhaustiveness test in `tests` will fail.
pub const STORAGE_KEY_NAMESPACE: u32 = 1;

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
/// Stored in persistent ledger storage keyed by [`DataKey::Transaction`].
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

// ─── Upgrade quorum (#87) ─────────────────────────────────────────────────────

/// Optional M-of-N co-signer set required for [`crate::SynapseCoreContract::upgrade`].
///
/// Distinct from the single admin key: when configured, admin auth alone is
/// rejected and at least [`Self::threshold`] distinct members of
/// [`Self::members`] must co-sign on-chain. `None` (absent storage) preserves
/// today's single-admin behaviour for backward compatibility.
#[contracttype]
#[derive(Clone, Debug)]
pub struct UpgradeQuorum {
    /// Minimum number of distinct member co-signatures required (M).
    pub threshold: u32,
    /// Designated co-signer set (N). Must be non-empty; threshold ∈ `[1, N]`.
    pub members: Vec<Address>,
}

// ─── Storage keys ─────────────────────────────────────────────────────────────

/// Logical storage discriminants (un-versioned).
///
/// Always wrap with [`StorageKey::ns`] before touching the ledger. See
/// [`STORAGE_KEY_NAMESPACE`] for the namespacing convention (#89).
#[contracttype]
#[derive(Clone, Debug)]
pub enum DataKey {
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
    /// Singleton: optional upgrade co-signer quorum (#87). Absent = single-admin.
    UpgradeQuorum,
    /// Singleton: WASM hash most recently upgraded *from* (#90).
    /// Absent until the first successful `upgrade()`.
    PreviousWasmHash,
    /// Singleton: WASM hash the contract is currently running (#90 helper).
    /// Set at `initialize()` to the genesis hash; updated on every upgrade.
    CurrentWasmHash,
    /// Singleton: pending upgrade proposal when using the multi-step path (#87).
    PendingUpgrade,
    /// Accumulated co-signer approvals for [`DataKey::PendingUpgrade`] (#87).
    UpgradeApprovals,
}

/// Versioned ledger storage key (#89).
///
/// Every on-chain entry is `(namespace, DataKey)`. Construct via
/// [`StorageKey::ns`] so the namespace cannot drift from
/// [`STORAGE_KEY_NAMESPACE`].
#[contracttype]
#[derive(Clone, Debug)]
pub enum StorageKey {
    /// Namespaced key: `(STORAGE_KEY_NAMESPACE, logical discriminant)`.
    Ns(u32, DataKey),
}

impl StorageKey {
    /// Build a namespaced storage key under [`STORAGE_KEY_NAMESPACE`].
    pub fn ns(key: DataKey) -> Self {
        StorageKey::Ns(STORAGE_KEY_NAMESPACE, key)
    }
}

/// Pre-namespacing storage keys (schema version 1).
///
/// Retained solely so [`crate::SynapseCoreContract::migrate_storage_keys`] can
/// read legacy entries and rewrite them under [`StorageKey`]. Not used for new
/// writes. Mainnet cutover is a deployment-ops concern; this is the tooling.
#[contracttype]
#[derive(Clone, Debug)]
pub enum LegacyStorageKey {
    Initialised,
    Admin,
    RelaySigner,
    Paused,
    Transaction(String),
    IdempotencyKey(String),
    PendingAdmin,
    SchemaVersion,
}

/// Pending upgrade proposal awaiting quorum co-signatures (#87).
#[contracttype]
#[derive(Clone, Debug)]
pub struct PendingUpgrade {
    pub new_wasm_hash: BytesN<32>,
    pub expected_schema_version: u32,
    pub proposer: Address,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

/// All error codes returned by the contract.
///
/// Uses [`contracterror`] so they surface correctly via the Soroban XDR and
/// can be decoded by SDK clients / frontends.
#[contracterror]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum ContractError {
    // ── Initialisation ──────────────────────────────────────────────────────
    /// `initialize()` has already been called.
    AlreadyInitialised = 1,
    /// Contract has not yet been initialised.
    NotInitialised = 2,

    // ── Authorisation ───────────────────────────────────────────────────────
    /// Caller is not the admin.
    Unauthorised = 10,
    /// Caller is not the trusted relay signer.
    NotRelaySigner = 11,
    /// The contract is paused (emergency circuit breaker engaged); the
    /// requested operation is temporarily disabled.
    ContractPaused = 12,
    /// `accept_admin` was called with no pending admin transfer in progress.
    NoPendingAdminTransfer = 13,
    /// `propose_admin` was called with the contract's own address as the
    /// nominee, which cannot practically call `accept_admin` back and would
    /// permanently brick every admin-gated operation.
    InvalidAdminNominee = 14,

    // ── Payload validation ──────────────────────────────────────────────────
    /// `stellar_account` field is malformed.
    InvalidStellarAccount = 20,
    /// `amount` is zero or negative.
    InvalidAmount = 21,
    /// `asset_code` is empty or exceeds 12 characters.
    InvalidAssetCode = 22,
    /// `asset_issuer` is malformed.
    InvalidAssetIssuer = 23,
    /// `idempotency_key` is empty.
    MissingIdempotencyKey = 24,
    /// A `String` field exceeds its maximum allowed length (cost-control cap).
    StringTooLong = 25,

    // ── Transaction lifecycle ───────────────────────────────────────────────
    /// No transaction with the given ID exists in storage.
    TransactionNotFound = 30,
    /// The requested status transition violates the state machine.
    InvalidStatusTransition = 31,

    // ── Idempotency ─────────────────────────────────────────────────────────
    /// Request is a duplicate within the retention window (matches Redis 429).
    DuplicateRequest = 40,

    // ── Storage ─────────────────────────────────────────────────────────────
    /// A ledger read/write produced an unexpected result.
    StorageError = 50,
    /// `migrate_storage_keys` found nothing to migrate, or schema already
    /// at the current namespaced layout.
    NothingToMigrate = 51,
    /// `migrate_storage_keys` refused: on-chain schema version is not the
    /// legacy (pre-namespace) version this migrator understands.
    UnexpectedSchemaForMigration = 52,

    // ── Upgrade safety ──────────────────────────────────────────────────────
    /// `upgrade()`'s `expected_schema_version` argument did not match the
    /// on-chain [`DataKey::SchemaVersion`]; the upgrade was aborted before
    /// touching contract WASM.
    SchemaVersionMismatch = 60,
    /// Upgrade quorum is configured but fewer than `threshold` distinct
    /// member co-signatures were supplied (#87).
    InsufficientUpgradeQuorum = 61,
    /// `set_upgrade_quorum` rejected: empty members, zero threshold, or
    /// threshold greater than member count.
    InvalidUpgradeQuorum = 62,
    /// `approve_upgrade` / related path called with no pending proposal.
    NoPendingUpgrade = 63,
    /// Co-signer address is not in the configured upgrade quorum set.
    NotUpgradeQuorumMember = 64,
    /// `CurrentWasmHash` was never recorded (initialize must supply the
    /// genesis WASM hash) so provenance cannot be written (#90).
    MissingCurrentWasmHash = 65,
}
