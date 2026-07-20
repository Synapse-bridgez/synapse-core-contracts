//! # Validation
//!
//! Stateless guards that reject malformed inputs before they touch storage.
//! Each function is pure (takes `&Env` only for string construction) and maps
//! directly to a [`ContractError`] variant.

use soroban_sdk::{Env, String};

use crate::types::{CallbackPayload, ContractError};

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
}
