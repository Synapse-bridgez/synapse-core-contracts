//! Shared types, error codes, and storage keys for the Anchor Registry contract.
//!
//! This module is the single source of truth for:
//! - [`ContractError`] — the contract's error enum, kept within issue #15's
//!   50-variant budget.
//! - [`StorageKey`] — the typed keys used to address persistent storage.
//! - Shared constants such as the default global amount ceiling.
//!
//! # Amount ceilings
//!
//! The contract enforces two layers of amount ceilings on every
//! `register_callback` / `batch_register_callback` validation pass:
//!
//! 1. A per-anchor override, stored under [`StorageKey::AnchorAmountCeiling`].
//! 2. A contract-wide default, stored under [`StorageKey::GlobalMaxAmount`]
//!    (instance tier), used whenever no per-anchor override is set.
//!
//! Both paths share the single [`ContractError::AmountCeilingExceeded`]
//! variant to conserve the error budget; callers that need to distinguish
//! the two layers should consult storage directly.

use soroban_sdk::{contracterror, contracttype, Address, BytesN, String};

/// Current storage schema version.
///
/// Must be kept consistent with the manifest. Bump this whenever the
/// layout of persistent/instance storage changes in a backwards-incompatible
/// way.
pub const SCHEMA_VERSION: u32 = 1;

/// Default contract-wide maximum transaction amount, in the smallest unit
/// of the relevant asset.
///
/// Used as the effective ceiling when the admin has never explicitly set
/// [`StorageKey::GlobalMaxAmount`]. "Unset" therefore means "use this sane
/// positive default" rather than "no ceiling" (which would be an economic
/// risk) or "zero" (which would silently reject everything).
pub const DEFAULT_GLOBAL_MAX_AMOUNT: i128 = 1_000_000_000_000;

/// Errors returned by the Anchor Registry contract.
///
/// The variant set is deliberately kept within the 50-variant budget
/// established by issue #15.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    // --- Generic / lifecycle (1-9) ---
    /// The contract has not been initialized.
    NotInitialized = 1,
    /// The contract has already been initialized.
    AlreadyInitialized = 2,
    /// The contract is paused.
    Paused = 3,
    /// The caller is not authorized to perform this action.
    Unauthorized = 4,
    /// The requested entry was not found.
    NotFound = 5,
    /// The requested entry already exists.
    AlreadyExists = 6,
    /// The operation is not supported.
    NotSupported = 7,
    /// An arithmetic operation overflowed.
    Overflow = 8,
    /// A re-entrant or concurrent modification was detected.
    Reentrancy = 9,

    // --- Input validation (10-19) ---
    /// A required parameter was missing or empty.
    MissingParam = 10,
    /// A parameter had an invalid value (e.g. a non-positive ceiling).
    InvalidParamValue = 11,
    /// An amount was invalid (e.g. non-positive or malformed).
    InvalidAmount = 12,
    /// An address was invalid.
    InvalidAddress = 13,
    /// A signature was invalid.
    InvalidSignature = 14,
    /// A timestamp was invalid.
    InvalidTimestamp = 15,
    /// A string exceeded its maximum permitted length.
    StringTooLong = 16,
    /// A batch exceeded its maximum permitted size.
    BatchTooLarge = 17,
    /// A batch was empty.
    BatchEmpty = 18,
    /// A duplicate entry was found within a batch.
    DuplicateInBatch = 19,

    // --- Amount ceilings (20-24) ---
    /// The transaction amount exceeded the effective amount ceiling.
    ///
    /// This variant is shared by both ceiling layers: the per-anchor
    /// override (when set) and the global default ceiling (when no override
    /// exists). Callers that need to distinguish the two should read the
    /// relevant storage entries directly.
    AmountCeilingExceeded = 20,

    // --- Anchor lifecycle (25-34) ---
    /// The anchor is not active.
    AnchorInactive = 25,
    /// The anchor has been suspended.
    AnchorSuspended = 26,
    /// The anchor has been revoked.
    AnchorRevoked = 27,
    /// The anchor is already registered.
    AnchorAlreadyRegistered = 28,
    /// The anchor is not registered.
    AnchorNotRegistered = 29,
    /// The anchor's quota has been exhausted.
    QuotaExhausted = 30,
    /// The anchor's rate limit was exceeded.
    RateLimitExceeded = 31,
    /// The anchor's tier does not permit this operation.
    TierNotPermitted = 32,
    /// The anchor's jurisdiction is not supported.
    JurisdictionNotSupported = 33,
    /// The anchor's asset is not supported.
    AssetNotSupported = 34,

    // --- Callback / relay (35-44) ---
    /// The callback was not found.
    CallbackNotFound = 35,
    /// The callback has already been registered.
    CallbackAlreadyRegistered = 36,
    /// The callback payload was malformed.
    MalformedCallback = 37,
    /// The relay signer set is empty.
    RelaySignerSetEmpty = 38,
    /// The relay signer is not in the authorized set.
    RelaySignerNotAuthorized = 39,
    /// The relay signer set has reached its maximum size.
    RelaySignerSetFull = 40,
    /// The nonce has already been used.
    NonceAlreadyUsed = 41,
    /// The nonce is out of sequence.
    NonceOutOfSequence = 42,
    /// The dispute is not open.
    DisputeNotOpen = 43,
    /// The dispute has already been resolved.
    DisputeAlreadyResolved = 44,

    // --- Storage / migration (45-50) ---
    /// A storage entry was missing or corrupt.
    StorageCorrupt = 45,
    /// The storage schema version does not match.
    SchemaVersionMismatch = 46,
    /// A migration is already in progress.
    MigrationInProgress = 47,
    /// The migration target version is invalid.
    InvalidMigrationTarget = 48,
    /// The operation would exceed a configured limit.
    LimitExceeded = 49,
    /// An unexpected internal error occurred.
    InternalError = 50,
}

/// Typed keys used to address contract storage.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageKey {
    // --- Instance tier ---
    /// Contract initialization flag.
    Initialized,
    /// Contract pause flag.
    Paused,
    /// The authorized relay signer set.
    RelaySignerSet,
    /// The contract-wide default maximum transaction amount (issue #49).
    ///
    /// Stored in the instance tier. When absent, the effective ceiling is
    /// [`DEFAULT_GLOBAL_MAX_AMOUNT`].
    GlobalMaxAmount,

    // --- Persistent tier ---
    /// Per-anchor record, keyed by anchor address.
    Anchor(Address),
    /// Per-anchor amount-ceiling override (issue #48), keyed by anchor
    /// address. When absent, the anchor falls back to the global default
    /// ceiling.
    AnchorAmountCeiling(Address),
    /// Per-anchor callback record, keyed by (anchor, callback id).
    Callback(Address, BytesN<32>),
    /// Per-anchor callback count.
    CallbackCount(Address),
    /// Per-anchor dispute record, keyed by (anchor, dispute id).
    Dispute(Address, BytesN<32>),
    /// Used nonce, keyed by nonce value.
    Nonce(BytesN<32>),
    /// Per-anchor metadata string.
    AnchorMetadata(Address, String),
}