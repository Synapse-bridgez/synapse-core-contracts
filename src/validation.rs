use crate::errors::ContractError;

/// Validates that `amount` does not exceed an already-resolved ceiling value.
///
/// This is the single shared ceiling check used by both the per-anchor ceiling
/// path (issue #48) and the global-default ceiling path (issue #49). The logic
/// that decides *which* ceiling applies — per-anchor override vs. global
/// default — lives in the storage layer (`storage`/`lib.rs`). This function is
/// intentionally minimal: it only checks a given `amount` against a given,
/// already-resolved `resolved_ceiling` number.
///
/// # Arguments
///
/// * `amount` - The amount to validate.
/// * `resolved_ceiling` - The ceiling value already resolved by the caller
///   (either the per-anchor override or the global default).
///
/// # Returns
///
/// * `Ok(())` if `amount <= resolved_ceiling`.
/// * `Err(ContractError::AmountExceedsCeiling)` if `amount > resolved_ceiling`.
///
/// # Examples
///
/// ```ignore
/// // Caller resolves the ceiling first (storage layer responsibility).
/// let ceiling = resolve_ceiling(&env, &anchor);
/// validate_amount_ceiling(amount, ceiling)?;
/// ```
pub fn validate_amount_ceiling(amount: i128, resolved_ceiling: i128) -> Result<(), ContractError> {
    if amount > resolved_ceiling {
        return Err(ContractError::AmountExceedsCeiling);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amount_below_ceiling_is_ok() {
        assert_eq!(validate_amount_ceiling(50, 100), Ok(()));
    }

    #[test]
    fn amount_at_ceiling_is_ok() {
        assert_eq!(validate_amount_ceiling(100, 100), Ok(()));
    }

    #[test]
    fn amount_above_ceiling_is_err() {
        assert_eq!(
            validate_amount_ceiling(101, 100),
            Err(ContractError::AmountExceedsCeiling)
        );
    }

    #[test]
    fn zero_ceiling_allows_zero_amount() {
        assert_eq!(validate_amount_ceiling(0, 0), Ok(()));
    }

    #[test]
    fn zero_ceiling_rejects_positive_amount() {
        assert_eq!(
            validate_amount_ceiling(1, 0),
            Err(ContractError::AmountExceedsCeiling)
        );
    }
}