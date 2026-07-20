//! # Validation
//!
//! Stateless guards that reject malformed inputs before they touch storage.
//! Each function is pure (takes `&Env` only for string construction) and maps
//! directly to a [`ContractError`] variant.
//!
//! ## String-length caps
//!
//! Several `String` fields have maximum-length caps to prevent unbounded
//! storage-rent costs.  These caps are enforced here and referenced by the
//! [`cost model`](../COST_MODEL.md#6-string-length-cap-impact).

use soroban_sdk::{Env, String};

use crate::types::{CallbackPayload, ContractError};

/// Maximum length (in bytes) for a `transaction_id` field (UUIDv4 is 36 chars).
const MAX_TX_ID_LEN: u32 = 64;
/// Maximum length for `anchor_transaction_id` (opaque AP ID, typically ≤ 36).
const MAX_ANCHOR_TX_ID_LEN: u32 = 64;
/// Maximum length for `callback_status` (short code, e.g. "pending_external").
const MAX_CALLBACK_STATUS_LEN: u32 = 32;
/// Maximum length for `stellar_tx_hash` (SHA-256 hex is 64 chars).
const MAX_STELLAR_TX_HASH_LEN: u32 = 72;
/// Maximum length for `failure_reason` (short human-readable code).
const MAX_FAILURE_REASON_LEN: u32 = 64;

/// Generic helper: reject a `String` if its byte length exceeds `max`.
fn enforce_max_length(
    field: &String,
    max: u32,
) -> Result<(), ContractError> {
    if field.len() > max {
        return Err(ContractError::StringTooLong);
    }
    Ok(())
}

pub struct Validator;

impl Validator {
    /// Validate an incoming [`CallbackPayload`] before writing to ledger.
    ///
    /// Runs every sub-check and returns the first error encountered.
    pub fn validate_payload(env: &Env, payload: &CallbackPayload) -> Result<(), ContractError> {
        Self::validate_stellar_account(env, &payload.stellar_account)?;
        Self::validate_amount(payload.amount)?;
        Self::validate_asset_code(env, &payload.asset_code)?;
        Self::validate_asset_issuer(env, &payload.asset_issuer)?;
        Self::validate_idempotency_key(env, &payload.idempotency_key)?;
        Self::validate_transaction_id(&payload.transaction_id)?;
        Self::validate_anchor_transaction_id(&payload.anchor_transaction_id)?;
        Self::validate_callback_status(&payload.callback_status)?;
        Ok(())
    }

    /// Stellar G-address: must be exactly 56 characters and start with 'G'.
    pub fn validate_stellar_account(_env: &Env, account: &String) -> Result<(), ContractError> {
        if account.len() != 56 {
            return Err(ContractError::InvalidStellarAccount);
        }
        let mut buf = [0u8; 56];
        account.copy_into_slice(&mut buf);
        if buf[0] != b'G' {
            return Err(ContractError::InvalidStellarAccount);
        }
        Ok(())
    }

    /// Amount must be strictly positive (> 0 stroops).
    pub fn validate_amount(amount: i128) -> Result<(), ContractError> {
        if amount <= 0 {
            return Err(ContractError::InvalidAmount);
        }
        Ok(())
    }

    /// Asset code: 1–12 ASCII uppercase characters (SEP-11).
    pub fn validate_asset_code(_env: &Env, code: &String) -> Result<(), ContractError> {
        let len = code.len() as usize;
        if len == 0 || len > 12 {
            return Err(ContractError::InvalidAssetCode);
        }
        let mut buf = [0u8; 12];
        code.copy_into_slice(&mut buf[..len]);
        for &byte in &buf[..len] {
            if !byte.is_ascii_uppercase() {
                return Err(ContractError::InvalidAssetCode);
            }
        }
        Ok(())
    }

    /// Issuer: must be a valid G-address (same rule as stellar_account).
    pub fn validate_asset_issuer(env: &Env, issuer: &String) -> Result<(), ContractError> {
        Self::validate_stellar_account(env, issuer)
            .map_err(|_| ContractError::InvalidAssetIssuer)
    }

    /// Idempotency key: non-empty (off-chain enforces UUID format).
    pub fn validate_idempotency_key(_env: &Env, key: &String) -> Result<(), ContractError> {
        if key.is_empty() {
            return Err(ContractError::MissingIdempotencyKey);
        }
        Ok(())
    }

    /// Transaction ID: UUID format expected; max length enforced for rent cost
    /// control.
    pub fn validate_transaction_id(id: &String) -> Result<(), ContractError> {
        enforce_max_length(id, MAX_TX_ID_LEN)
    }

    /// Anchor transaction ID: max length enforced for rent cost control.
    pub fn validate_anchor_transaction_id(id: &String) -> Result<(), ContractError> {
        enforce_max_length(id, MAX_ANCHOR_TX_ID_LEN)
    }

    /// Callback status: max length enforced for rent cost control.
    pub fn validate_callback_status(status: &String) -> Result<(), ContractError> {
        enforce_max_length(status, MAX_CALLBACK_STATUS_LEN)
    }

    /// Stellar transaction hash: max length enforced for rent cost control.
    pub fn validate_stellar_tx_hash(hash: &String) -> Result<(), ContractError> {
        enforce_max_length(hash, MAX_STELLAR_TX_HASH_LEN)
    }

    /// Failure reason: max length enforced for rent cost control.
    pub fn validate_failure_reason(reason: &String) -> Result<(), ContractError> {
        enforce_max_length(reason, MAX_FAILURE_REASON_LEN)
    }
}
