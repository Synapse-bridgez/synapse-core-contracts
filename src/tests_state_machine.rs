//! State-machine tests for the transaction lifecycle.
//!
//! Covers `start_processing` (Pending -> Processing) and the shared
//! transition guard used by the lifecycle entry points. Every illegal
//! source-state transition is exercised explicitly, alongside the happy
//! path and the auth-failure case.

use crate::{
    ContractError, TransactionStatus, Transaction, TransactionClient,
};
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

/// Helper: register the contract and return a client plus an admin address.
fn setup<'a>(env: &'a Env) -> (TransactionClient<'a>, Address) {
    let contract_id = env.register_contract(None, crate::Contract);
    let client = TransactionClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (client, admin)
}

/// Helper: create a transaction in `Pending` and return its id.
fn create_pending(client: &TransactionClient, env: &Env, caller: &Address) -> u64 {
    let recipient = Address::generate(env);
    client.create_transaction(caller, &recipient, &1_000i128)
}

#[test]
fn start_processing_happy_path() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let tx_id = create_pending(&client, &env, &admin);

    // Admin (relay-or-admin gated) moves Pending -> Processing.
    client.start_processing(&tx_id, &admin);

    let tx: Transaction = client.get_transaction(&tx_id);
    assert_eq!(tx.status, TransactionStatus::Processing);
}

#[test]
fn start_processing_by_relay_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let relay = Address::generate(&env);
    client.set_relay(&admin, &relay);

    let tx_id = create_pending(&client, &env, &admin);
    client.start_processing(&tx_id, &relay);

    let tx: Transaction = client.get_transaction(&tx_id);
    assert_eq!(tx.status, TransactionStatus::Processing);
}

#[test]
fn start_processing_emits_status_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let tx_id = create_pending(&client, &env, &admin);
    client.start_processing(&tx_id, &admin);

    // The `status` topic event must be emitted on success.
    let events = env.events().all();
    let status_topic = Symbol::new(&env, "status");
    let found = events.iter().any(|(_, topics, _)| {
        topics.iter().any(|t| {
            // Topics are encoded; compare against the expected symbol.
            t == &status_topic.into()
        })
    });
    assert!(found, "expected a `status` topic event to be emitted");
}

#[test]
fn start_processing_auth_failure() {
    let env = Env::default();
    // Do NOT mock auths: an unauthorized caller must be rejected.
    let contract_id = env.register_contract(None, crate::Contract);
    let client = TransactionClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin);
    let tx_id = create_pending(&client, &env, &admin);

    // Reset auth mocking so the next call is unauthorized.
    env.set_auths(&[]);
    let stranger = Address::generate(&env);
    let result = client.try_start_processing(&tx_id, &stranger);
    assert!(result.is_err(), "unauthorized caller must be rejected");
}

#[test]
fn start_processing_nonexistent_tx_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let result = client.try_start_processing(&999u64, &admin);
    assert_eq!(result, Err(Ok(ContractError::TransactionNotFound)));
}

#[test]
fn start_processing_from_processing_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let tx_id = create_pending(&client, &env, &admin);
    client.start_processing(&tx_id, &admin);

    let result = client.try_start_processing(&tx_id, &admin);
    assert_eq!(result, Err(Ok(ContractError::InvalidStatusTransition)));
}

#[test]
fn start_processing_from_completed_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let tx_id = create_pending(&client, &env, &admin);
    client.start_processing(&tx_id, &admin);
    client.complete_transaction(&tx_id, &admin);

    let result = client.try_start_processing(&tx_id, &admin);
    assert_eq!(result, Err(Ok(ContractError::InvalidStatusTransition)));
}

#[test]
fn start_processing_from_failed_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let tx_id = create_pending(&client, &env, &admin);
    client.start_processing(&tx_id, &admin);
    client.fail_transaction(&tx_id, &admin);

    let result = client.try_start_processing(&tx_id, &admin);
    assert_eq!(result, Err(Ok(ContractError::InvalidStatusTransition)));
}

#[test]
fn start_processing_from_cancelled_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let tx_id = create_pending(&client, &env, &admin);
    client.cancel_transaction(&tx_id, &admin);

    let result = client.try_start_processing(&tx_id, &admin);
    assert_eq!(result, Err(Ok(ContractError::InvalidStatusTransition)));
}