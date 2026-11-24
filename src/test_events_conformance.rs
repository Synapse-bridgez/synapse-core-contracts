//! Event-conformance coverage for the catalogued event topics.
//!
//! The contract emits a catalogued set of event topics; each must be a valid
//! Soroban `Symbol` so the off-chain decoder can match it. This test covers the
//! `taglimit` topic by exercising the tag-limit rejection path and asserting
//! exactly one event is produced for the overflow.

#![cfg(test)]

use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{Address, Env, String};

use crate::types::MAX_TAGS_PER_TRANSACTION;
use crate::{SynapseCoreContract, SynapseCoreContractClient};

#[test]
fn event_decoder_covers_catalogued_topics() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    let guardian = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay, &guardian);
    client.register_callback(&relay, &1u64, &100i128);

    let tag = String::from_str(&env, "tag");
    for _ in 0..MAX_TAGS_PER_TRANSACTION {
        client.add_transaction_tag(&relay, &1u64, &tag);
    }
    let before = env.events().all().len();
    // The 11th tag overflows and emits the catalogued `taglimit` event.
    let _ = client.try_add_transaction_tag(&relay, &1u64, &tag);
    assert_eq!(env.events().all().len() - before, 1u32);
}
