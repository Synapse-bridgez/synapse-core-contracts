//! Boundary tests for the per-transaction tag limit (issue #471).
//!
//! When a transaction exceeds `MAX_TAGS_PER_TRANSACTION` the rejection emits a
//! `taglimit` event carrying the supplied count and the limit, then returns
//! `ContractError::TagLimitExceeded`, so a client can render "you sent N,
//! limit is M".

#![cfg(test)]

use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{Address, Env, String};

use crate::types::{ContractError, MAX_TAGS_PER_TRANSACTION};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

fn setup() -> (Env, SynapseCoreContractClient<'static>, Address, Address) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    let guardian = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay, &guardian);
    (env, client, admin, relay)
}

#[test]
fn tags_up_to_limit_are_accepted() {
    let (env, client, _admin, relay) = setup();
    client.register_callback(&relay, &1u64, &100i128);
    let tag = String::from_str(&env, "tag");
    for _ in 0..MAX_TAGS_PER_TRANSACTION {
        assert_eq!(
            client.try_add_transaction_tag(&relay, &1u64, &tag),
            Ok(Ok(()))
        );
    }
}

#[test]
fn tag_over_limit_is_rejected_and_emits_event() {
    let (env, client, _admin, relay) = setup();
    client.register_callback(&relay, &1u64, &100i128);
    let tag = String::from_str(&env, "tag");
    for _ in 0..MAX_TAGS_PER_TRANSACTION {
        client.add_transaction_tag(&relay, &1u64, &tag);
    }
    let before = env.events().all().len();
    let result = client.try_add_transaction_tag(&relay, &1u64, &tag);
    assert_eq!(result, Err(Ok(ContractError::TagLimitExceeded)));
    // The overflow emits exactly one catalogued `taglimit` event.
    assert_eq!(env.events().all().len() - before, 1u32);
}

#[test]
fn tag_on_unknown_transaction_is_not_found() {
    let (env, client, _admin, relay) = setup();
    let tag = String::from_str(&env, "tag");
    let result = client.try_add_transaction_tag(&relay, &999u64, &tag);
    assert_eq!(result, Err(Ok(ContractError::NotFound)));
}
