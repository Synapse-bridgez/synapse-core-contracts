//! Integration tests for `synapse-core-contract`.
//!
//! Run with:
//! ```bash
//! cargo test
//! ```
//!
//! Each `#[test]` corresponds to one contract entry-point and should cover:
//! * happy path
//! * authorisation failure
//! * invalid input
//! * duplicate / idempotency path
//! * invalid state-machine transition

#![cfg(test)]

extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, String,
};

use synapse_core_contract::{SynapseCoreContract, SynapseCoreContractClient};
use synapse_core_contract::types::{CallbackPayload, CallbackType, ContractError, TransactionStatus};

// ─── Test helpers ─────────────────────────────────────────────────────────────

fn setup() -> (Env, SynapseCoreContractClient<'static>) {
    // TODO: let env = Env::default();
    // TODO: env.mock_all_auths();
    // TODO: let contract_id = env.register_contract(None, SynapseCoreContract);
    // TODO: let client = SynapseCoreContractClient::new(&env, &contract_id);
    // TODO: (env, client)
    todo!()
}

fn default_payload(env: &Env) -> CallbackPayload {
    // TODO: build a valid CallbackPayload with test fixtures
    todo!()
}

// ─── initialize() ─────────────────────────────────────────────────────────────

#[test]
fn test_initialize_happy_path() {
    // TODO: setup → initialize(admin, relay) → assert health() == true
    todo!()
}

#[test]
fn test_initialize_rejects_double_init() {
    // TODO: initialize twice → assert Err(AlreadyInitialised)
    todo!()
}

// ─── register_callback() ──────────────────────────────────────────────────────

#[test]
fn test_register_callback_creates_pending_transaction() {
    // TODO: register → get_transaction → assert status == Pending
    todo!()
}

#[test]
fn test_register_callback_idempotency_returns_same_id() {
    // TODO: register same payload twice → second call returns same tx_id
    todo!()
}

#[test]
fn test_register_callback_rejects_non_relay_caller() {
    // TODO: call with a random address → assert Err(NotRelaySigner)
    todo!()
}

#[test]
fn test_register_callback_rejects_invalid_account() {
    // TODO: payload with bad stellar_account → assert Err(InvalidStellarAccount)
    todo!()
}

#[test]
fn test_register_callback_rejects_zero_amount() {
    // TODO: payload with amount = 0 → assert Err(InvalidAmount)
    todo!()
}

// ─── start_processing() ───────────────────────────────────────────────────────

#[test]
fn test_start_processing_pending_to_processing() {
    // TODO: register → start_processing → assert status == Processing
    todo!()
}

#[test]
fn test_start_processing_rejects_non_pending() {
    // TODO: register → complete → start_processing → Err(InvalidStatusTransition)
    todo!()
}

// ─── complete_transaction() ───────────────────────────────────────────────────

#[test]
fn test_complete_transaction_happy_path() {
    // TODO: register → start_processing → complete → assert status == Completed
    // TODO: assert stellar_tx_hash stored correctly
    todo!()
}

#[test]
fn test_complete_transaction_rejects_pending() {
    // TODO: register → complete (skipping Processing) → Err(InvalidStatusTransition)
    todo!()
}

// ─── fail_transaction() ───────────────────────────────────────────────────────

#[test]
fn test_fail_transaction_from_pending() {
    // TODO: register → fail → assert status == Failed, reason stored
    todo!()
}

#[test]
fn test_fail_transaction_from_processing() {
    // TODO: register → start_processing → fail → assert Failed
    todo!()
}

#[test]
fn test_fail_transaction_rejects_completed() {
    // TODO: drive to Completed → fail → Err(InvalidStatusTransition)
    todo!()
}

// ─── Admin ────────────────────────────────────────────────────────────────────

#[test]
fn test_transfer_admin_happy_path() {
    // TODO: transfer_admin(new_admin) → assert new_admin can call privileged ops
    todo!()
}

#[test]
fn test_transfer_admin_rejects_non_admin() {
    // TODO: random caller → Err(Unauthorised)
    todo!()
}

#[test]
fn test_set_relay_signer_happy_path() {
    // TODO: set_relay_signer(new_relay) → old relay can no longer register callbacks
    todo!()
}

// ─── Events ───────────────────────────────────────────────────────────────────

#[test]
fn test_register_emits_transaction_registered_event() {
    // TODO: register → env.events().all() → assert contains TransactionRegistered
    todo!()
}

#[test]
fn test_complete_emits_status_changed_and_completed_events() {
    // TODO: drive to Completed → assert StatusChanged + TransactionCompleted events
    todo!()
}
