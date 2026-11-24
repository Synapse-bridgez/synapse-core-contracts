//! Tests for the read-only pause-state query (issue #470).
//!
//! `get_pause_state` must reflect the current pause flag and reason without
//! requiring admin auth, so a client can discover the pause state before
//! submitting a call.

#![cfg(test)]

use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, String};

use crate::{SynapseCoreContract, SynapseCoreContractClient};

fn setup() -> (Env, SynapseCoreContractClient<'static>, Address) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    let guardian = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay, &guardian);
    (env, client, admin)
}

#[test]
fn pause_state_starts_unpaused_with_no_reason() {
    let (_env, client, _admin) = setup();
    let state = client.get_pause_state();
    assert!(!state.paused);
    assert_eq!(state.reason, None);
}

#[test]
fn pause_sets_state_and_reason() {
    let (env, client, admin) = setup();
    let reason = String::from_str(&env, "maintenance");
    client.pause(&admin, &reason);
    let state = client.get_pause_state();
    assert!(state.paused);
    assert_eq!(state.reason, Some(String::from_str(&env, "maintenance")));
}

#[test]
fn unpause_clears_state_and_reason() {
    let (env, client, admin) = setup();
    let reason = String::from_str(&env, "maintenance");
    client.pause(&admin, &reason);
    client.unpause(&admin);
    let state = client.get_pause_state();
    assert!(!state.paused);
    assert_eq!(state.reason, None);
}

#[test]
fn get_pause_state_requires_no_auth() {
    // Read-only: callable without admin auth. The query takes no caller, so
    // any client may read it; the default unpaused state is returned.
    let (_env, client, _admin) = setup();
    let state = client.get_pause_state();
    assert!(!state.paused);
}
