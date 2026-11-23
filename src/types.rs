//! # Types
//!
//! On-chain equivalents of the `synapse-core` Rust service's domain model.
//! Every struct that touches ledger storage derives [`soroban_sdk::contracttype`].

use soroban_sdk::{contracterror, contracttype, String};

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
    /// Transaction was cancelled before completing.
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
    pub anchor_transaction_id: String,

    /// `deposit` or `withdrawal`.
    pub callback_type: CallbackType,

    /// Raw status string received from the Anchor Platform.
    pub callback_status: String,

    /// Stellar transaction hash recorded after on-chain settlement.
    pub stellar_tx_hash: String,

    /// Short failure reason code — populated only on `Failed`.
    pub failure_reason: String,
}

// ─── Incoming webhook payload ─────────────────────────────────────────────────

/// Payload forwarded by the trusted relay signer when calling
/// [`SynapseCoreContract::register_callback`].
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

    /// Idempotency key for deduplication.
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
#[contracttype]
#[derive(Clone, Debug)]
pub enum StorageKey {
    /// Singleton: whether `initialize()` has been called.
    Initialised,
    /// Singleton: current admin address.
    Admin,
    /// Singleton: trusted relay signer address.
    RelaySigner,
    /// Per-transaction record keyed by transaction ID.
    Transaction(String),
    /// Idempotency key → cached response ledger; keyed by idempotency key.
    IdempotencyKey(String),
}

// ─── Treasury config ──────────────────────────────────────────────────────────

/// Treasury configuration used by the two-party withdrawal flow (issue #284).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreasuryConfig {
    /// First authorized party.
    pub party_a: soroban_sdk::Address,
    /// Second authorized party.
    pub party_b: soroban_sdk::Address,
    /// Maximum amount withdrawable per epoch.
    pub epoch_cap: i128,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

/// All error codes returned by the contract.
///
/// Uses [`contracterror`] so they surface correctly via the Soroban XDR and
/// can be decoded by SDK clients / frontends. Discriminant values are stable
/// across upgrades; do not renumber existing variants.
///
/// ## Error-code budget (issue #15 / DECISIONS.md)
///
/// Soroban caps a `#[contracterror]` enum at 50 variants. The ranges below
/// are reserved to stay within that cap and to keep related errors adjacent
/// so callers can range-check categories.
#[contracterror]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum ContractError {
    // ── Initialisation (1–9) ─────────────────────────────────────────────────
    /// `initialize()` has already been called.
    AlreadyInitialised = 1,
    /// Contract has not yet been initialised.
    NotInitialised = 2,

    // ── Authorisation (10–19) ────────────────────────────────────────────────
    /// Caller is not the admin.
    Unauthorised = 10,
    /// Caller is not the trusted relay signer.
    NotRelaySigner = 11,

    // ── Payload validation (20–29) ───────────────────────────────────────────
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
    /// A free-form string field exceeds the maximum allowed length.
    StringTooLong = 25,
    /// `payload` is otherwise invalid.
    InvalidPayload = 26,

    // ── Transaction lifecycle (30–39) ────────────────────────────────────────
    /// No transaction with the given ID exists in storage.
    TransactionNotFound = 30,
    /// The requested status transition violates the state machine.
    InvalidStatusTransition = 31,
    /// The transaction cannot be cancelled in its current state.
    CannotCancel = 32,
    /// The transaction has already been cancelled.
    AlreadyCancelled = 33,
    /// The retry limit for this transaction has been exceeded.
    RetryLimitExceeded = 34,
    /// The transaction has already been merged into another.
    AlreadyMerged = 35,
    /// Cannot merge a transaction into itself.
    MergeSelf = 36,
    /// The transaction has already been settled (duplicate settlement attempt).
    DuplicateSettled = 37,
    /// The amount ceiling configured for the contract would be exceeded.
    AmountCeilingExceeded = 38,

    // ── Idempotency (40–44) ──────────────────────────────────────────────────
    /// Request is a duplicate within the retention window.
    DuplicateRequest = 40,
    /// Batch size is zero or above the configured maximum.
    InvalidBatchSize = 41,
    /// Batch budget (conservative resource estimate) would be exceeded.
    BatchBudgetExceeded = 42,

    // ── Storage / internal (45–49) ───────────────────────────────────────────
    /// A ledger read/write produced an unexpected result.
    StorageError = 45,
    /// Arithmetic overflow or underflow.
    ArithmeticOverflow = 46,
    /// A self-consistency check failed.
    SelfCheckFailed = 47,

    // ── Admin transfer (50–54) ───────────────────────────────────────────────
    /// There is no pending admin transfer to accept.
    NoPendingAdminTransfer = 50,
    /// The proposed new admin address is invalid (e.g. the contract itself).
    InvalidAdminNominee = 51,

    // ── Contract lifecycle (55–59) ───────────────────────────────────────────
    /// The contract is paused; this entry point is blocked.
    ContractPaused = 55,
    /// The schema version expected by the caller does not match the live version.
    SchemaVersionMismatch = 56,
    /// A timelock is required before this action can proceed.
    TimelockRequired = 57,
    /// The timelock has not elapsed yet.
    TimelockNotElapsed = 58,
    /// There is no pending change to act on.
    NoPendingChange = 59,

    // ── Upgrade / migration (60–64) ──────────────────────────────────────────
    /// The referenced migration identifier is unknown.
    UnknownMigration = 60,
    /// The migration failed to apply.
    MigrationFailed = 61,
    /// The upgrade cannot be reversed.
    UpgradeNotReversible = 62,
    /// There is nothing to roll back.
    NothingToRollback = 63,

    // ── Bonding / collateral (65–69) ─────────────────────────────────────────
    /// The signer's bond balance is insufficient for the requested operation.
    InsufficientBond = 65,
    /// The bond amount supplied is invalid (e.g. zero).
    InvalidBondAmount = 66,
    /// The signer has no bond on record.
    SignerNotBonded = 67,
    /// An unbond request is already pending for this signer.
    UnbondAlreadyPending = 68,
    /// No pending unbond request exists for this signer.
    NoPendingUnbond = 69,
    /// The unbonding delay ledgers have not elapsed yet.
    UnbondDelayNotElapsed = 70,
    /// The slash evidence submitted is invalid.
    InvalidSlashEvidence = 71,

    // ── Relay quorum (72–74) ─────────────────────────────────────────────────
    /// The quorum threshold has not been met.
    QuorumNotMet = 72,
    /// The relay threshold value is invalid.
    InvalidThreshold = 73,
    /// The relay delay value is invalid.
    InvalidDelay = 74,

    // ── Pagination (75–76) ───────────────────────────────────────────────────
    /// The pagination limit is zero or exceeds the maximum.
    InvalidPageLimit = 75,

    // ── Anchor tiers / params (77–81) ────────────────────────────────────────
    /// No anchor tier exists for the requested label.
    AnchorTierNotFound = 77,
    /// The tier label is empty or exceeds the maximum length.
    InvalidTierLabel = 78,
    /// The rebate basis-points value is out of range.
    InvalidRebateBps = 79,
    /// The param name is empty or exceeds the maximum length.
    InvalidParamName = 80,
    /// The param value is out of the accepted range.
    InvalidParamValue = 81,
    /// No param exists for the requested name.
    ParamNotFound = 82,

    // ── Treasury withdrawal (83–89) ──────────────────────────────────────────
    /// Treasury has not been configured.
    TreasuryNotConfigured = 83,
    /// The withdrawal amount is zero or negative.
    InvalidWithdrawalAmount = 84,
    /// Treasury balance is insufficient to satisfy the withdrawal request.
    ///
    /// Callers can distinguish this from a storage failure by checking for
    /// this specific code. See issue #447.
    InsufficientTreasuryBalance = 85,
    /// The withdrawal amount would exceed the per-epoch cap.
    WithdrawalCapExceeded = 86,
    /// No pending withdrawal proposal exists.
    NoPendingWithdrawal = 87,
    /// The authorizing relay signer does not match the one that proposed.
    WithdrawalNotRelaySigner = 88,
    /// The withdrawal proposal details do not match what was proposed.
    WithdrawalProposalMismatch = 89,
}
