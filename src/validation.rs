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
    pub fn validate_stellar_account(env: &Env, account: &String) -> Result<(), ContractError> {
        // TODO: account.len() == 56 && account starts with b'G'
        // Use env.bytes() comparison rather than std string ops.
        todo!()
    }

    /// Amount must be strictly positive (> 0 stroops).
    pub fn validate_amount(amount: i128) -> Result<(), ContractError> {
        // TODO: if amount <= 0 { return Err(ContractError::InvalidAmount) }
        todo!()
    }

    /// Asset code: 1–12 ASCII uppercase characters (SEP-11).
    pub fn validate_asset_code(env: &Env, code: &String) -> Result<(), ContractError> {
        // TODO: 1 <= code.len() <= 12, all chars uppercase ASCII
        todo!()
    }

    /// Issuer: must be a valid G-address (same rule as stellar_account).
    pub fn validate_asset_issuer(env: &Env, issuer: &String) -> Result<(), ContractError> {
        // TODO: reuse validate_stellar_account logic
        todo!()
    }

    /// Idempotency key: non-empty (off-chain enforces UUID format).
    pub fn validate_idempotency_key(env: &Env, key: &String) -> Result<(), ContractError> {
        // TODO: if key.len() == 0 { return Err(ContractError::MissingIdempotencyKey) }
        todo!()
    }
}
