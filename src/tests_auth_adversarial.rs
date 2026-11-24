//! Adversarial auth tests asserting each admin guard returns a distinct,
//! documented error code (issue #469).
//!
//! `require_admin`, `require_relay_or_admin`, `require_pending_admin`, and
//! `require_guardian` each fail with their own code, so a caller (or assistive
//! tech) can tell which precondition was unmet.

#![cfg(test)]

use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

use crate::types::ContractError;
use crate::{SynapseCoreContract, SynapseCoreContractClient};

fn setup() -> (
    Env,
    SynapseCoreContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    let guardian = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay, &guardian);
    (env, client, admin, relay, guardian)
}

#[test]
fn require_admin_failure_returns_not_admin() {
    let (env, client, _admin, _relay, _guardian) = setup();
    let stranger = Address::generate(&env);
    let new_relay = Address::generate(&env);
    let result = client.try_set_relay_signer(&stranger, &new_relay);
    assert_eq!(result, Err(Ok(ContractError::NotAdmin)));
}

#[test]
fn require_relay_or_admin_failure_returns_not_relay_or_admin() {
    let (env, client, _admin, _relay, _guardian) = setup();
    let stranger = Address::generate(&env);
    let result = client.try_register_callback(&stranger, &1u64, &100i128);
    assert_eq!(result, Err(Ok(ContractError::NotRelayOrAdmin)));
}

#[test]
fn require_pending_admin_failure_returns_not_pending_admin() {
    let (env, client, admin, _relay, _guardian) = setup();
    let nominee = Address::generate(&env);
    client.propose_admin(&admin, &nominee);
    let stranger = Address::generate(&env);
    let result = client.try_accept_admin(&stranger);
    assert_eq!(result, Err(Ok(ContractError::NotPendingAdmin)));
}

#[test]
fn require_guardian_failure_returns_not_guardian() {
    let (env, client, _admin, _relay, _guardian) = setup();
    let stranger = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let result = client.try_guardian_revoke_admin(&stranger, &new_admin);
    assert_eq!(result, Err(Ok(ContractError::NotGuardian)));
}

#[test]
fn admin_can_call_admin_gated() {
    let (env, client, admin, _relay, _guardian) = setup();
    let new_relay = Address::generate(&env);
    assert_eq!(client.try_set_relay_signer(&admin, &new_relay), Ok(Ok(())));
}
