//! # Types
//!
//! On-chain equivalents of the `synapse-core` Rust service's domain model.
//! Every struct that touches ledger storage derives [`soroban_sdk::contracttype`].

use soroban_sdk::{contracterror, contracttype, Address, String};

/// Current on-chain storage schema version.
///
/// Bump this whenever [`Transaction`] or [`StorageKey`] layout changes in a
/// way that a running upgrade needs to be aware of. `initialize()` stores it;
/// `SynapseCoreContract::upgrade()` requires the caller to pass the value it
/// currently expects on-chain before proceeding (`THREAT_MODEL.md` finding
/// F-04). This cannot validate the *new* WASM's schema — Soroban gives the
/// currently-running code no way to introspect an uploaded-but-not-yet-
/// installed WASM blob — so it guards against upgrading the wrong deployment
/// or an unexpected on-chain state, not against an incompatible new binary.
pub const SCHEMA_VERSION: u32 = 1;

/// Hard cap on the number of payloads accepted by `batch_register_callback`.
pub const MAX_BATCH_SIZE: u32 = 20;

/// Maximum `limit` accepted by paginated read-only queries.
pub const MAX_PAGE_LIMIT: u32 = 50;

/// Contract-wide maximum registrable amount used while the
/// `global_max_amount` param has never been set: 10^15 stroops
/// (100M units at 7 decimals). Makes the global ceiling an always-on
/// backstop, even on a freshly initialised contract.
pub const DEFAULT_GLOBAL_MAX_AMOUNT: i128 = 1_000_000_000_000_000;

/// Maximum number of simultaneously open disputes. Bounds the size of the
/// single persistent entry backing `get_dispute_queue`.
pub const MAX_OPEN_DISPUTES: u32 = 100;

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

    /// Deposit amount in stroops (1 XLM = `10_000_000` stroops).
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
    /// (e.g. `"pending_external"`, `"completed"`).
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

    // ── Amount ceilings (#169) ───────────────────────────────────────────────
    /// Per-anchor maximum registrable amount, keyed by the payload's
    /// `asset_issuer`. Absent means only the global ceiling applies.
    AnchorCeiling(String),

    // ── Disputes (#166) ──────────────────────────────────────────────────────
    /// Per-transaction open-dispute record. Present only while the
    /// transaction is under dispute.
    Dispute(String),
    /// Singleton: open disputes in the order they were raised (oldest first).
    DisputeQueue,
    /// Singleton: monotonically increasing dispute sequence counter.
    DisputeSeq,
}

// ─── Wave 2: Param Registry (#146) ────────────────────────────────────────────

/// A single on-chain parameter entry.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::Param`].
/// All tunable values (fee rate, unbond delay, slash percentage, fee ceiling,
/// etc.) live here rather than as independent ad-hoc admin-settable fields.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParamEntry {
    /// Param value. Represented as `i128` to accommodate both integer counts
    /// and scaled basis-point rates (e.g. `9_500` = 95.00 %).
    pub value: i128,
    /// Ledger sequence at which this param was last updated.
    pub updated_at_ledger: u32,
    /// Address that last set this param (always the admin).
    pub updated_by: soroban_sdk::Address,
}

// ─── Wave 2: Collateral Bonding (#143) ────────────────────────────────────────

/// Collateral bond record for a relay signer.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::BondRecord`].
#[contracttype]
#[derive(Clone, Debug)]
pub struct BondRecord {
    /// The signer whose collateral is bonded.
    pub signer: soroban_sdk::Address,
    /// Amount currently bonded (in stroops / token's smallest unit).
    pub amount: i128,
    /// Ledger sequence at which the current bond was first created.
    pub bonded_at_ledger: u32,
    /// Ledger sequence of the most recent top-up (same as `bonded_at_ledger`
    /// if no top-up has occurred since the initial bond).
    pub updated_at_ledger: u32,
}

/// Pending unbond request from a relay signer.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::UnbondRequest`].
/// The existence of this record is the proof that an unbond has been requested
/// but has not yet been claimed (i.e. the delay has not yet elapsed).
#[contracttype]
#[derive(Clone, Debug)]
pub struct UnbondRequest {
    /// Amount requested to unbond.
    pub amount: i128,
    /// Ledger sequence at which the unbond was requested.
    pub requested_at_ledger: u32,
    /// Ledger sequence at which the unbond may be claimed (= `requested_at` +
    /// the `unbond_delay_ledgers` param at request time).
    pub claimable_at_ledger: u32,
}

// ─── Wave 2: Slash Evidence (#144) ────────────────────────────────────────────

/// On-chain-provable evidence of misbehaviour: two conflicting signed callbacks
/// for the same `transaction_id`.
///
/// This is the only misbehaviour type in Wave 2 — deliberately scoped to what
/// is mechanically, non-subjectively verifiable on-chain without any off-chain
/// judgement call:
///
/// * Both payloads' `transaction_id` fields must equal `tx_id`.
/// * At least one field *other* than `idempotency_key` must differ.
///
/// The `slash_signer` handler verifies these conditions and rejects the call if
/// any check fails.
#[contracttype]
#[derive(Clone, Debug)]
pub struct SlashEvidence {
    /// The `transaction_id` that appears in both conflicting callbacks.
    pub tx_id: String,
    /// Payload of callback A (the one already on-chain in storage).
    pub payload_a: CallbackPayload,
    /// Payload of callback B (the conflicting second callback).
    pub payload_b: CallbackPayload,
}

// ─── Wave 2: Anchor Tier / Rebate (#145) ──────────────────────────────────────

/// Trust/volume tier config for a single anchor address.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::AnchorTier`].
/// The admin sets this to indicate that a particular anchor qualifies for a
/// reduced effective fee rate.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorTierConfig {
    /// Anchor address this tier applies to.
    pub anchor: soroban_sdk::Address,
    /// Rebate in basis points (`0–10_000`).
    ///
    /// `0`     = no rebate (full fee rate applies).
    /// `500`   = 5 % rebate.
    /// `10_000` = 100 % rebate (zero effective fee).
    ///
    /// Effective fee = `base_fee_bps` × (`10_000` − `rebate_bps`) / `10_000`.
    pub rebate_bps: u32,
    /// Human-readable label for the tier (e.g. "gold", "silver", "standard").
    /// Max 16 chars; purely informational.
    pub label: String,
    /// Ledger sequence at which this tier was last set.
    pub updated_at_ledger: u32,
}

// ─── Disputes (#166) ──────────────────────────────────────────────────────────

/// An open dispute against a `Completed` transaction.
///
/// Stored in persistent storage keyed by [`StorageKey::Dispute`] and removed
/// when the dispute is resolved.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisputeRecord {
    /// Position in the dispute queue; also the `get_dispute_queue` cursor.
    pub seq: u64,
    /// Short reason code supplied by the caller.
    pub reason: String,
    /// Relay signer or admin that raised the dispute.
    pub raised_by: Address,
    /// Ledger sequence at which the dispute was raised.
    pub raised_at_ledger: u32,
    /// Ledger timestamp (seconds) at which the dispute was raised.
    pub raised_at_timestamp: u64,
}

/// One entry of the open-dispute queue, ordered by `seq` (oldest first).
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisputeQueueEntry {
    pub seq: u64,
    pub tx_id: String,
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

    // ── Upgrade safety ──────────────────────────────────────────────────────
    /// `upgrade()`'s `expected_schema_version` argument did not match the
    /// on-chain [`SchemaVersion`](StorageKey::SchemaVersion); the upgrade was
    /// aborted before touching contract WASM.
    SchemaVersionMismatch = 60,

    // ── Wave 2: Param Registry (#146) ───────────────────────────────────────
    /// `set_param` was called with an empty param name.
    InvalidParamName = 70,
    /// `set_param` value is outside the allowed range for that parameter.
    InvalidParamValue = 71,
    /// `get_param` was called for a param that has never been set.
    ParamNotFound = 72,

    // ── Wave 2: Collateral Bonding (#143) ───────────────────────────────────
    /// `bond_collateral` amount is zero or negative.
    InvalidBondAmount = 80,
    /// `unbond_collateral` requested more than the currently bonded balance.
    InsufficientBond = 81,
    /// `unbond_collateral` was called while a previous unbond request is still
    /// pending (i.e. not yet claimed).
    UnbondAlreadyPending = 82,
    /// `claim_unbond` was called before the unbond delay has elapsed.
    UnbondDelayNotElapsed = 83,
    /// `claim_unbond` was called but no pending unbond request exists.
    NoPendingUnbond = 84,

    // ── Wave 2: Slashing (#144) ─────────────────────────────────────────────
    /// `slash_signer` evidence `tx_id` does not match both payloads'
    /// `transaction_id` fields.
    EvidenceTxIdMismatch = 90,
    /// `slash_signer` evidence payloads are identical (no conflicting content).
    EvidenceNotConflicting = 91,
    /// `slash_signer` was called for a signer with no bonded collateral.
    SignerNotBonded = 92,

    // ── Wave 2: Anchor Rebate (#145) ────────────────────────────────────────
    /// `set_anchor_tier` `rebate_bps` exceeds `10_000` (100 %).
    InvalidRebateBps = 100,
    /// `set_anchor_tier` label exceeds the maximum allowed length.
    InvalidTierLabel = 101,
    /// `get_anchor_tier` / `compute_effective_fee` anchor has no tier set.
    AnchorTierNotFound = 102,

    // ── Batch registration (#173) ───────────────────────────────────────────
    /// A batch was empty or exceeded `MAX_BATCH_SIZE`.
    InvalidBatchSize = 110,
    /// A batch's conservative worst-case resource estimate exceeds the safety
    /// budget, independent of its item count. Rejected before any write.
    BatchBudgetExceeded = 111,

    // ── Disputes (#166) ─────────────────────────────────────────────────────
    /// Transaction is already under dispute.
    AlreadyDisputed = 120,
    /// Transaction is not under dispute.
    NotDisputed = 121,
    /// `MAX_OPEN_DISPUTES` disputes are already open.
    DisputeQueueFull = 122,
    /// A pagination `limit` was zero or exceeded `MAX_PAGE_LIMIT`.
    InvalidPageLimit = 123,

    // ── Amount ceilings (#169) ──────────────────────────────────────────────
    /// Amount exceeds the stricter of the per-anchor and global ceilings.
    AmountCeilingExceeded = 130,
}
