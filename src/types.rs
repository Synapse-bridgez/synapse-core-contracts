    // --- Invalid input (1-9) ---
    /// Contract has already been initialized.
    AlreadyInitialized = 1,
    /// Contract has not been initialized yet.
    NotInitialized = 2,
    /// Caller supplied malformed or out-of-range input.
    InvalidInput = 3,
#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    // --- Invalid input (1-9) ---
    /// Contract has already been initialized.
    AlreadyInitialized = 1,
    /// Contract has not been initialized yet.
    NotInitialized = 2,
    /// Caller supplied malformed or out-of-range input.
    InvalidInput = 3,
    /// Schema compatibility range is invalid.
    InvalidSchemaCompatRange = 4,
    /// Stored schema version does not match the expected version.
    SchemaVersionMismatch = 5,

    // --- Unauthorized (10-14) ---
    /// Caller is not authorized for this operation.
    Unauthorized = 10,
    /// Operation is forbidden in the current mode (e.g. paused).
    Forbidden = 11,

    // --- State machine (15-29) ---
    /// The operation cannot be cancelled in the current state.
    CannotCancel = 15,
    /// The operation has already been cancelled.
    AlreadyCancelled = 16,
    /// The state machine forbids this transition.
    InvalidTransition = 17,
    /// The timelock has not elapsed yet.
    TimelockNotElapsed = 18,
    /// There is no pending change to act on.
    NoPendingChange = 19,
    /// There is nothing to roll back.
    NothingToRollback = 20,

    // --- Migration/upgrade (30-39) ---
    /// The referenced migration is unknown.
    UnknownMigration = 30,
    /// The migration failed to apply.
    MigrationFailed = 31,
    /// The upgrade cannot be reversed.
    UpgradeNotReversible = 32,

    // --- Operational (40-50) ---
    /// The retry limit has been exceeded.
    RetryLimitExceeded = 40,
    /// A self-consistency check failed.
    SelfCheckFailed = 41,
    /// Arithmetic overflow or underflow.
    Overflow = 42,
    /// A required record was not found.
    NotFound = 43,
    /// A record already exists.
    AlreadyExists = 44,
    /// Underlying storage error.
    StorageError = 45,
    /// Unexpected internal error.
    InternalError = 46,
}
