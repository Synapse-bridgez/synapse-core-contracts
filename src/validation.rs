//! Validation helpers for the relay contract.
//!
//! Guard functions are grouped into buckets. Bucket 5 holds the quorum /
//! threshold bounds checks used by `set_relay_threshold` and its
//! `try_set_relay_threshold` counterpart.

use soroban_sdk::{contracterror, panic_with_error, Env};

/// Errors surfaced by the validation guards.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
#[allow(dead_code)]
pub enum ValidationError {
    /// The supplied threshold was zero, which would make quorum trivially met.
    ThresholdZero = 1,
    /// The supplied threshold exceeded the current signer-set size, which
    /// would make quorum permanently unreachable.
    ThresholdExceedsSignerCount = 2,
}

/// Validates that `threshold` is a reachable, non-trivial quorum for a signer
/// set of `signer_count` members.
///
/// A threshold of `0` is rejected because it would make quorum trivially
/// always met. A threshold greater than `signer_count` is rejected because it
/// would make quorum permanently unreachable. A threshold exactly equal to
/// `signer_count` is valid (full consensus required).
///
/// Returns `Ok(())` when the threshold is within bounds, otherwise the
/// corresponding [`ValidationError`].
#[allow(dead_code)]
pub fn validate_threshold_bounds(
    threshold: u32,
    signer_count: u32,
) -> Result<(), ValidationError> {
    if threshold == 0 {
        return Err(ValidationError::ThresholdZero);
    }
    if threshold > signer_count {
        return Err(ValidationError::ThresholdExceedsSignerCount);
    }
    Ok(())
}

/// Panicking wrapper around [`validate_threshold_bounds`].
///
/// Used by the `set_relay_threshold` entry point so that an out-of-bounds
/// threshold aborts the invocation with a typed contract error.
#[allow(dead_code)]
pub fn require_threshold_bounds(env: &Env, threshold: u32, signer_count: u32) {
    if let Err(err) = validate_threshold_bounds(threshold, signer_count) {
        panic_with_error!(env, err);
    }
}
