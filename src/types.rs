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

// ─── Transaction status ───────────────────────────────────────────────────────

/// Mirrors the `status` column in the `transactions` table.
///
/// State machine:
/// ```text
/// Pending ──► Processing ──► Completed
///         └──────────────► Failed
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
    Failed,
    /// Cancelled by relay or admin before completion (stub for forward-looking tests).
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

    /// Short failure reason code — populated only on `Failed`.
    pub failure_reason: String,

    /// Number of times this transaction has been retried after failure.
    /// Zero on first registration. Incremented by `retry_transaction`.
    pub retry_count: u32,
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

// ─── Bit-packed status + flags (#117) ────────────────────────────────────────

/// Compact encoding of [`TransactionStatus`] (plus room for future boolean
/// flags) into a single `u32`.
///
/// A `#[contracttype]` unit enum is serialised as an `ScVec` holding the
/// variant name as an `ScSymbol` (e.g. `["Processing"]`), which costs far more
/// ledger bytes than a single `ScU32`. [`StoredTransaction`] persists this
/// packed word instead, and [`crate::storage::StorageClient`] converts at the
/// storage boundary so the public API still sees [`Transaction::status`].
///
/// ## Layout
///
/// ```text
/// bits [31 .. 8]  — reserved (written as 0)
/// bits [ 7 .. 4]  — status nibble (0=Pending, 1=Processing, 2=Completed,
///                                  3=Failed, 4=Cancelled; 5..=15 unassigned)
/// bits [ 3 .. 0]  — reserved flag bits (written as 0)
/// ```
///
/// `Transaction` has no boolean flags today, so every flag bit is reserved.
/// Adding a flag in a reserved bit is additive: [`Self::unpack_status`]
/// ignores reserved bits, so older code still decodes the status. Changing the
/// status nibble mapping is a schema-breaking change and requires a
/// [`SCHEMA_VERSION`] bump.
pub struct StatusFlags;

impl StatusFlags {
    /// Bit offset of the status nibble.
    pub const STATUS_SHIFT: u32 = 4;
    /// Mask selecting the status nibble.
    pub const STATUS_MASK: u32 = 0xF << Self::STATUS_SHIFT;

    /// Encode a [`TransactionStatus`] into the compact `u32` representation.
    #[inline]
    pub fn pack(status: TransactionStatus) -> u32 {
        let nibble: u32 = match status {
            TransactionStatus::Pending => 0,
            TransactionStatus::Processing => 1,
            TransactionStatus::Completed => 2,
            TransactionStatus::Failed => 3,
            TransactionStatus::Cancelled => 4,
        };
        nibble << Self::STATUS_SHIFT
    }

    /// Decode the status nibble of a packed `u32`.
    ///
    /// Reserved bits are ignored. Returns `None` if the nibble is unassigned
    /// (e.g. written by a future schema version with more status variants).
    #[inline]
    pub fn unpack_status(packed: u32) -> Option<TransactionStatus> {
        let nibble = (packed & Self::STATUS_MASK) >> Self::STATUS_SHIFT;
        match nibble {
            0 => Some(TransactionStatus::Pending),
            1 => Some(TransactionStatus::Processing),
            2 => Some(TransactionStatus::Completed),
            3 => Some(TransactionStatus::Failed),
            4 => Some(TransactionStatus::Cancelled),
            _ => None,
        }
    }
}

/// Ledger representation of a [`Transaction`] (#117).
///
/// Identical to [`Transaction`] except that `status` is replaced by the
/// [`StatusFlags`]-packed `status_flags` word. Only
/// [`crate::storage::StorageClient`] reads or writes this type; everything
/// else, including every public entry point, uses [`Transaction`].
#[contracttype(export = false)]
#[derive(Clone, Debug)]
pub struct StoredTransaction {
    pub id: String,
    pub stellar_account: String,
    pub amount: i128,
    pub asset_code: String,
    pub asset_issuer: String,
    /// [`StatusFlags`]-packed status (and future flag bits).
    pub status_flags: u32,
    pub created_at_ledger: u32,
    pub updated_at_ledger: u32,
    pub anchor_transaction_id: String,
    pub callback_type: CallbackType,
    pub callback_status: String,
    pub stellar_tx_hash: String,
    pub failure_reason: String,
    pub retry_count: u32,
}

impl StoredTransaction {
    /// Pack a [`Transaction`] for storage.
    pub fn pack(tx: &Transaction) -> Self {
        StoredTransaction {
            id: tx.id.clone(),
            stellar_account: tx.stellar_account.clone(),
            amount: tx.amount,
            asset_code: tx.asset_code.clone(),
            asset_issuer: tx.asset_issuer.clone(),
            status_flags: StatusFlags::pack(tx.status),
            created_at_ledger: tx.created_at_ledger,
            updated_at_ledger: tx.updated_at_ledger,
            anchor_transaction_id: tx.anchor_transaction_id.clone(),
            callback_type: tx.callback_type.clone(),
            callback_status: tx.callback_status.clone(),
            stellar_tx_hash: tx.stellar_tx_hash.clone(),
            failure_reason: tx.failure_reason.clone(),
            retry_count: tx.retry_count,
        }
    }

    /// Unpack into the public [`Transaction`] shape.
    ///
    /// Returns `None` if `status_flags` carries an unassigned status nibble.
    pub fn unpack(self) -> Option<Transaction> {
        Some(Transaction {
            status: StatusFlags::unpack_status(self.status_flags)?,
            id: self.id,
            stellar_account: self.stellar_account,
            amount: self.amount,
            asset_code: self.asset_code,
            asset_issuer: self.asset_issuer,
            created_at_ledger: self.created_at_ledger,
            updated_at_ledger: self.updated_at_ledger,
            anchor_transaction_id: self.anchor_transaction_id,
            callback_type: self.callback_type,
            callback_status: self.callback_status,
            stellar_tx_hash: self.stellar_tx_hash,
            failure_reason: self.failure_reason,
            retry_count: self.retry_count,
        })
    }
}

// ─── Storage keys ─────────────────────────────────────────────────────────────

/// Typed discriminants for all named singleton keys in persistent/instance
/// storage. Used by [`StorageKey::ns`] to build a namespaced key.
///
/// Exists as a separate enum (rather than plain symbols) so the compiler
/// catches typos and tests can refer to key names without string literals.
/// `DataKey` is the *logical* name; [`StorageKey`] carries the *storage* value.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataKey {
    Initialised,
    Admin,
    RelaySigner,
    Paused,
    PendingAdmin,
    SchemaVersion,
}

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

    // ── Amount ceiling (#116 / amount-ceiling guard) ──────────────────────────
    /// Per-anchor amount ceiling (in stroops). Keyed by asset_issuer address
    /// string. Used by `validate_amount_ceiling` in the hot path.
    AmountCeiling(String),

    // ── Stub variants for forward-looking features ────────────────────────────
    /// Stores the WASM hash of the currently installed contract code.
    /// Referenced by the upgrade machinery and simulate_upgrade stubs.
    CurrentWasmHash,
}

impl StorageKey {
    /// Convert a [`DataKey`] singleton discriminant into its corresponding
    /// [`StorageKey`] variant, enabling tests to reference named storage entries
    /// without duplicating the mapping.
    ///
    /// Used in tests via `env.storage().persistent().extend_ttl(&StorageKey::ns(DataKey::Admin), ...)`.
    pub fn ns(key: DataKey) -> StorageKey {
        match key {
            DataKey::Initialised => StorageKey::Initialised,
            DataKey::Admin => StorageKey::Admin,
            DataKey::RelaySigner => StorageKey::RelaySigner,
            DataKey::Paused => StorageKey::Paused,
            DataKey::PendingAdmin => StorageKey::PendingAdmin,
            DataKey::SchemaVersion => StorageKey::SchemaVersion,
        }
    }
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

// ─── Stub types for forward-looking upgrade features ─────────────────────────
//
// These types are referenced by test files written ahead of the implementing
// issues (simulate_upgrade, upgrade history, etc.). They compile but are not
// yet fully wired into live contract logic.

/// Maximum number of upgrade history records retained on-chain.
#[allow(dead_code)] // used once upgrade history is restored (#199)
pub const MAX_UPGRADE_HISTORY: u32 = 10;

/// A single entry in the upgrade history ring buffer.
#[contracttype]
#[derive(Clone, Debug)]
pub struct UpgradeRecord {
    /// WASM hash that was installed before this upgrade.
    pub previous_wasm_hash: soroban_sdk::BytesN<32>,
    /// WASM hash installed by this upgrade.
    pub new_wasm_hash: soroban_sdk::BytesN<32>,
    /// Admin that authorised the upgrade.
    pub admin: soroban_sdk::Address,
    /// Ledger sequence at which the upgrade occurred.
    pub ledger: u32,
    /// Schema version at upgrade time.
    pub schema_version: u32,
}

/// Result of a `simulate_upgrade` dry-run check.
#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpgradeCompatibility {
    /// All pre-flight checks passed; a real upgrade would succeed.
    Compatible,
    /// `expected_schema_version` does not match the on-chain schema version.
    SchemaVersionMismatch,
    /// The caller is not the current admin.
    CallerNotAdmin,
    /// Contract has not been initialised.
    NotInitialised,
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
    /// `set_anchor_tier` rebate_bps exceeds 10_000 (100 %).
    InvalidRebateBps = 100,
    /// `set_anchor_tier` label exceeds the maximum allowed length.
    InvalidTierLabel = 101,
    /// `get_anchor_tier` / `compute_effective_fee` anchor has no tier set.
    AnchorTierNotFound = 102,

    // ── Wave 2: Amount ceiling / payload guards ──────────────────────────────
    /// `register_callback` amount exceeds the per-anchor ceiling.
    AmountCeilingExceeded = 110,
    /// A tag field is empty.
    EmptyTag = 111,
    /// Transaction already has the maximum number of tags.
    TooManyTags = 112,
    /// Partial settlement amount is out of the valid range
    /// (`0 < settled < original`).
    InvalidSettledAmount = 113,

    // ── Stub variants for forward-looking test coverage ──────────────────────
    // These variants are referenced by tests written ahead of their implementing
    // issues. They compile but no handler currently returns them.
    /// Transaction is in a terminal or already-cancelled state; cannot cancel.
    CannotCancel = 120,
    /// Transaction has already been cancelled.
    AlreadyCancelled = 121,
    /// Transaction has reached the maximum allowed retry count.
    RetryLimitExceeded = 122,
    /// `batch_register_callback` batch size exceeds the allowed limit.
    InvalidBatchSize = 123,
    /// `get_transactions_by_status` page limit is invalid.
    InvalidPageLimit = 124,
    /// Upgrade self-check failed after WASM installation.
    SelfCheckFailed = 125,
}
