//! Input validation helpers.
//!
//! `validate_amount_ceiling` rejects amounts above a resolved ceiling (the
//! existing transfer ceiling path). `validate_nonzero_amount` rejects zero
//! amounts with the dedicated `ContractError::ZeroAmount` code required by
//! issue #472, applied to both the transfer and treasury-withdrawal paths.

use crate::types::ContractError;

/// Reject `amount` above `resolved_ceiling`.
///
/// Returns `Ok(())` when `amount <= resolved_ceiling`, otherwise
/// `ContractError::AmountExceedsCeiling`.
pub fn validate_amount_ceiling(amount: i128, resolved_ceiling: i128) -> Result<(), ContractError> {
    if amount > resolved_ceiling {
        return Err(ContractError::AmountExceedsCeiling);
    }
    Ok(())
}

/// Reject a zero or non-positive `amount` (issue #472).
///
/// Returns `ContractError::ZeroAmount` when `amount <= 0`. Applied to both the
/// transfer (`register_callback`) and treasury-withdrawal paths so a client
/// can explain to a user why their transfer failed.
pub fn validate_nonzero_amount(amount: i128) -> Result<(), ContractError> {
    if amount <= 0 {
        return Err(ContractError::ZeroAmount);
    }
    Ok(())
}
