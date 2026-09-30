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
pub const SCHEMA_VERSION: u32 = 1;

/// Maximum number of times a `Failed` transaction may be retried.
pub const MAX_RETRIES: u32 = 3;

/// Hard cap on the `limit` accepted by `get_transactions_by_status`.
pub const MAX_PAGE_LIMIT: u32 = 50;

/// Hard cap on the number of payloads accepted by `batch_register_callback`.
pub const MAX_BATCH_SIZE: u32 = 20;

/// Maximum number of [`UpgradeRecord`]s retained by `get_upgrade_history`.
/// The oldest record is evicted (FIFO) once the cap is reached.
pub const MAX_UPGRADE_HISTORY: u32 = 32;

/// Default timelock for `propose_upgrade` → `finalize_upgrade` (~24h at
/// ~5s/ledger). Overridable via `set_upgrade_delay`.
pub const DEFAULT_UPGRADE_DELAY_LEDGERS: u32 = 17_280;

/// Default relay-signer rotation timelock: ~24h at ~5s/ledger.
pub const DEFAULT_RELAY_SIGNER_DELAY_LEDGERS: u32 = 17_280;

// ─── Transaction status ───────────────────────────────────────────────────────

/// Mirrors the `status` column in the `transactions` table.
///
/// State machine:
/// ```text
/// Pending ──► Processing ──► Completed
///   │             │
///   ├─────────────┴────────► Failed ──(retry)──► Pending
///   └─────────────┴────────► Cancelled
/// ```
#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransactionStatus {
    /// Initial state — callback received, not yet picked up by the processor.
    Pending,
    /// Off-chain processor has claimed the job; on-chain verification in progress.
    Processing,
    /// Stellar on-chain settlement confirmed; ready for Phase 2 (Swap Engine).
    Completed,
    /// Terminal failure — reason stored in [`Transaction::failure_reason`].
    /// May be moved back to `Pending` by `retry_transaction`.
    Failed,
    /// Terminal withdrawal — voided by the relay or admin before settlement.
    Cancelled,
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

    /// Short failure reason code — populated only on `Failed` / `Cancelled`.
    pub failure_reason: String,

    /// Number of times this transaction has been moved `Failed -> Pending`
    /// via `retry_transaction`. Capped at [`MAX_RETRIES`].
    pub retry_count: u32,

    /// Amount actually settled when completed via
    /// `partial_complete_transaction`; `None` for a full settlement.
    pub settled_amount: Option<i128>,
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

    // ── Transaction queries / metadata (#176, #177) ──────────────────────────
    /// Per-status index size: number of transactions currently in a status.
    StatusCount(TransactionStatus),
    /// Per-status index slot `(status, i)` -> transaction ID, `i < count`.
    /// Slots are dense; removal swaps the last slot into the gap so every
    /// index update is O(1) and no single ledger entry grows unboundedly.
    StatusSlot(TransactionStatus, u32),
    /// Reverse pointer: transaction ID -> its slot in its current status.
    StatusSlotOf(String),
    /// Per-transaction tag list (`Vec<String>`), kept out of the
    /// [`Transaction`] record so untagged transactions pay no extra rent.
    TxTags(String),

    // ── Amount ceilings (#178) ───────────────────────────────────────────────
    /// Per-anchor (asset issuer) maximum accepted callback amount.
    AmountCeiling(String),
    /// Singleton: contract-wide default ceiling for anchors without an entry.
    DefaultAmountCeiling,

    // ── Recovery / phase routing (#179) ──────────────────────────────────────
    /// Merge marker: duplicate tx id -> canonical tx id it was merged into.
    MergedInto(String),
    /// Per-transaction forwarding route (`next_phase`); absent = no forwarding.
    ForwardRoute(String),

    // ── Relay signer set / timelocked rotation (#179) ────────────────────────
    /// Singleton: N-of-M relay signer set ([`RelaySignerSet`]). Absent until
    /// the set is first changed; the single `RelaySigner` is then treated as
    /// `threshold = 1, signers = [relay_signer]`.
    RelaySignerSet,
    /// Temporary: a signer's standing approval for the next gated relay call.
    RelayApproval(Address),
    /// Singleton: pending timelocked relay-signer change.
    PendingRelaySigner,
    /// Singleton: relay-signer timelock delay in ledgers (absent = default).
    RelaySignerDelay,

    // ── Upgrade safety (#190, #191) ──────────────────────────────────────────
    /// Singleton: bounded `Vec<UpgradeRecord>` (see [`MAX_UPGRADE_HISTORY`]).
    UpgradeHistory,
    /// Singleton: WASM hash believed to be installed (seeded by
    /// `register_installed_wasm`, then updated by every upgrade).
    CurrentWasmHash,
    /// Singleton: [`UpgradeSnapshot`] of the pre-upgrade WASM, used by
    /// `rollback_upgrade`.
    PreviousUpgrade,
    /// Singleton: whether the last upgrade ran a storage migration.
    LastUpgradeMigrated,
    /// Singleton: scaffolding marker written by test migrations.
    MigrationMarker,
    /// Singleton: pending timelocked upgrade ([`PendingUpgrade`]).
    PendingUpgrade,
    /// Singleton: upgrade timelock delay in ledgers (absent = default).
    UpgradeDelay,
    /// Singleton: admin-declared [`SchemaCompatRange`] (absent = exact match).
    SchemaCompatRange,
}

// ─── Relay signer set (#179) ──────────────────────────────────────────────────

/// Pending timelocked relay-signer rotation, finalisable at `eta_ledger`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingRelaySigner {
    pub new_signer: Address,
    pub eta_ledger: u32,
}

/// N-of-M relay signer set: `threshold` distinct members must co-authorise
/// each relay-gated call. `signers[0]` is the primary signer returned by
/// `relay_signer()`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelaySignerSet {
    pub signers: Vec<Address>,
    pub threshold: u32,
}

// ─── Upgrade safety (#190, #191) ──────────────────────────────────────────────

/// One entry in the on-chain upgrade history (`get_upgrade_history`).
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeRecord {
    /// WASM hash before the upgrade (all zeroes if it was never registered).
    pub previous_wasm_hash: BytesN<32>,
    /// WASM hash installed by the upgrade.
    pub new_wasm_hash: BytesN<32>,
    /// On-chain schema version at the time of the upgrade.
    pub schema_version: u32,
    /// Ledger sequence of the upgrade.
    pub ledger: u32,
    /// Admin that authorised the upgrade.
    pub admin: Address,
}

/// The WASM + schema a `rollback_upgrade` would restore.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeSnapshot {
    pub wasm_hash: BytesN<32>,
    pub schema_version: u32,
}

/// A staged timelocked upgrade (`propose_upgrade`).
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingUpgrade {
    pub wasm_hash: BytesN<32>,
    pub expected_schema_version: u32,
    /// First ledger at which `finalize_upgrade` is legal.
    pub eta_ledger: u32,
}

/// Inclusive `[min, max]` window of `expected_schema_version` values the
/// upgrade entry points accept (ADR-0006).
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaCompatRange {
    pub min: u32,
    pub max: u32,
}

/// Verdict returned by the read-only `simulate_upgrade` dry-run. Mirrors the
/// guard order of the real `upgrade` entry point.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeCompatibility {
    /// Every guard `upgrade` checks before the WASM swap would pass.
    Compatible,
    /// The contract has not been initialised.
    NotInitialised,
    /// `caller` is not the current admin.
    CallerNotAdmin,
    /// `expected_schema_version` is outside the accepted range.
    SchemaVersionMismatch,
}

// ─── Wave 2: Param Registry (#146) ────────────────────────────────────────────

/// A single on-chain parameter entry.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::Param`].
/// All tunable values (fee rate, unbond delay, slash percentage, fee ceiling,
/// etc.) live here rather than as independent ad-hoc admin-settable fields.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ParamEntry {
    /// Param value. Represented as `i128` to accommodate both integer counts
    /// and scaled basis-point rates (e.g. 9_500 = 95.00 %).
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
    /// Ledger sequence at which the unbond may be claimed (= requested_at +
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
#[derive(Clone, Debug, PartialEq)]
pub struct AnchorTierConfig {
    /// Anchor address this tier applies to.
    pub anchor: soroban_sdk::Address,
    /// Rebate in basis points (0–10_000).
    ///
    /// `0`     = no rebate (full fee rate applies).
    /// `500`   = 5 % rebate.
    /// `10_000` = 100 % rebate (zero effective fee).
    ///
    /// Effective fee = base_fee_bps × (10_000 − rebate_bps) / 10_000.
    pub rebate_bps: u32,
    /// Human-readable label for the tier (e.g. "gold", "silver", "standard").
    /// Max 16 chars; purely informational.
    pub label: String,
    /// Ledger sequence at which this tier was last set.
    pub updated_at_ledger: u32,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

/// All error codes returned by the contract.
///
/// Uses [`contracterror`] so they surface correctly via the Soroban XDR and
/// can be decoded by SDK clients / frontends.
///
/// **Soroban caps a contract error enum at 50 variants** (the spec's
/// `cases<50>` XDR bound; the macro panics beyond it). New failure modes
/// should reuse an existing variant with a broadened meaning before adding
/// one — see DECISIONS.md "Error-code budget".
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
    /// An amount is out of range: a callback `amount` or amount ceiling that
    /// is zero or negative, or a partial `settled_amount` not strictly
    /// between zero and the registered amount.
    InvalidAmount = 21,
    /// `asset_code` is empty or exceeds 12 characters.
    InvalidAssetCode = 22,
    /// `asset_issuer` is malformed.
    InvalidAssetIssuer = 23,
    /// `idempotency_key` is empty.
    MissingIdempotencyKey = 24,
    /// A `String` field exceeds its maximum allowed length (cost-control cap).
    StringTooLong = 25,
    /// `amount` exceeds the per-anchor (or default) amount ceiling.
    AmountCeilingExceeded = 26,

    // ── Transaction lifecycle ───────────────────────────────────────────────
    /// No transaction with the given ID exists in storage.
    TransactionNotFound = 30,
    /// The requested status transition violates the state machine.
    InvalidStatusTransition = 31,
    /// The transaction is already `Cancelled`; cancelling twice is rejected.
    AlreadyCancelled = 32,
    /// Cancellation was requested from a state that cannot be cancelled
    /// (`Completed` or `Failed`).
    CannotCancel = 33,
    /// The transaction has already used all [`MAX_RETRIES`] retries.
    RetryLimitExceeded = 34,
    /// A pagination `limit` was zero or exceeded [`MAX_PAGE_LIMIT`].
    InvalidPageLimit = 35,
    /// A batch was empty or exceeded [`MAX_BATCH_SIZE`].
    InvalidBatchSize = 36,
    /// A transaction tag was empty, or the transaction already carries the
    /// maximum number of tags. (Over-long tags are `StringTooLong`.)
    InvalidTag = 37,

    // ── Idempotency ─────────────────────────────────────────────────────────
    /// Request is a duplicate within the retention window (matches Redis 429).
    DuplicateRequest = 40,

    // ── Upgrade safety ──────────────────────────────────────────────────────
    /// `upgrade()`'s `expected_schema_version` argument did not match the
    /// on-chain [`SchemaVersion`](StorageKey::SchemaVersion); the upgrade was
    /// aborted before touching contract WASM.
    SchemaVersionMismatch = 60,
    /// The post-upgrade storage self-check failed; the upgrade reverts.
    SelfCheckFailed = 61,
    /// A timelocked action (`finalize_upgrade`, `finalize_relay_signer`,
    /// `claim_unbond`) was attempted before its delay elapsed.
    TimelockNotElapsed = 62,
    /// A finalize / cancel call found no pending upgrade or relay-signer
    /// change.
    NoPendingChange = 63,
    /// A migration routine failed, or would exceed
    /// `MAX_MIGRATION_STORAGE_TOUCHES`; the whole upgrade reverts.
    MigrationFailed = 64,
    /// `upgrade_and_migrate` was given an unregistered `migration_id`.
    UnknownMigration = 65,
    /// `rollback_upgrade` with no recorded previous WASM.
    NothingToRollback = 67,
    /// The last upgrade migrated storage, so it cannot be rolled back.
    UpgradeNotReversible = 68,
    /// A schema compatibility range must satisfy `min <= current <= max`.
    InvalidSchemaCompatRange = 69,

    // ── Wave 2: Param Registry (#146) ───────────────────────────────────────
    /// `set_param` was called with an empty param name.
    InvalidParamName = 70,
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
    /// `claim_unbond` was called but no pending unbond request exists.
    NoPendingUnbond = 84,

    // ── Wave 2: Slashing (#144) ─────────────────────────────────────────────
    /// `slash_signer` evidence is invalid: its `tx_id` does not match both
    /// payloads' `transaction_id`, or the payloads do not conflict.
    InvalidSlashEvidence = 90,
    /// `slash_signer` was called for a signer with no bonded collateral.
    SignerNotBonded = 92,

    // ── Wave 2: Anchor Rebate (#145) ────────────────────────────────────────
    /// `set_anchor_tier` rebate_bps exceeds 10_000 (100 %).
    InvalidRebateBps = 100,
    /// `set_anchor_tier` label exceeds the maximum allowed length.
    InvalidTierLabel = 101,
    /// `get_anchor_tier` / `compute_effective_fee` anchor has no tier set.
    AnchorTierNotFound = 102,

    // ── Recovery / merge (#179) ─────────────────────────────────────────────
    /// `merge_duplicate_transactions` was given the same id for both sides.
    MergeSelf = 110,
    /// The duplicate (or canonical) record is already merged.
    AlreadyMerged = 111,
    /// The duplicate is `Completed` (settled); merging would be lossy.
    DuplicateSettled = 112,

    // ── Relay signer set (#179) ─────────────────────────────────────────────
    /// Threshold is 0 or exceeds the number of signers.
    InvalidThreshold = 120,
    /// Fewer than `threshold` distinct signers authorised the call.
    QuorumNotMet = 121,
    /// A non-zero relay-signer delay is configured; use propose/finalize.
    TimelockRequired = 122,
}
