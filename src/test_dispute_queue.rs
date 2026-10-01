//! Tests for disputes and the `get_dispute_queue` worklist query (#166).

#![cfg(test)]

extern crate std;

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, String, Vec,
};

use crate::types::{
    CallbackPayload, CallbackType, ContractError, TransactionStatus, MAX_OPEN_DISPUTES,
    MAX_PAGE_LIMIT,
};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

const G_ADDR: &str = "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ";

fn setup() -> (Env, SynapseCoreContractClient<'static>, Address) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    (env, client, relay)
}

/// Register `tx_id` and drive it to `Completed`.
fn completed(
    env: &Env,
    client: &SynapseCoreContractClient,
    relay: &Address,
    tx_id: &str,
) -> String {
    let account = String::from_str(env, G_ADDR);
    let id = client.register_callback(&CallbackPayload {
        transaction_id: String::from_str(env, tx_id),
        stellar_account: account.clone(),
        amount: 1_000,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: account,
        idempotency_key: String::from_str(env, tx_id),
        anchor_transaction_id: String::from_str(env, "anchor-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "completed"),
    });
    client.start_processing(&id, relay);
    client.complete_transaction(&id, &String::from_str(env, "hash"), relay);
    id
}

fn dispute(env: &Env, client: &SynapseCoreContractClient, relay: &Address, tx_id: &String) {
    client.dispute_transaction(tx_id, &String::from_str(env, "amount_mismatch"), relay);
}

fn ids(env: &Env, names: &[&str]) -> Vec<String> {
    let mut v = Vec::new(env);
    for n in names {
        v.push_back(String::from_str(env, n));
    }
    v
}

/// Advance the ledger so each dispute gets a distinct, later timestamp.
fn tick(env: &Env) {
    env.ledger().with_mut(|li| {
        li.timestamp += 60;
        li.sequence_number += 12;
    });
}

/// Four completed transactions, disputed in the non-sequential order C, A, D, B.
fn four_disputed(env: &Env, client: &SynapseCoreContractClient, relay: &Address) {
    let a = completed(env, client, relay, "A");
    let b = completed(env, client, relay, "B");
    let c = completed(env, client, relay, "C");
    let d = completed(env, client, relay, "D");
    for id in [&c, &a, &d, &b] {
        tick(env);
        dispute(env, client, relay, id);
    }
}

#[test]
fn queue_is_oldest_dispute_first_not_registration_order() {
    let (env, client, relay) = setup();
    four_disputed(&env, &client, &relay);

    let (page, next) = client.get_dispute_queue(&0, &MAX_PAGE_LIMIT);
    assert_eq!(page, ids(&env, &["C", "A", "D", "B"]));
    assert_eq!(next, None);

    // Raised timestamps are strictly increasing along the queue.
    let mut last = 0u64;
    for id in page.iter() {
        let rec = client.get_dispute(&id).unwrap();
        assert!(rec.raised_at_timestamp > last);
        last = rec.raised_at_timestamp;
    }
}

#[test]
fn resolved_disputes_leave_the_queue_immediately() {
    let (env, client, relay) = setup();
    four_disputed(&env, &client, &relay);

    // Rejected dispute: leaves the queue, transaction stays Completed.
    let a = String::from_str(&env, "A");
    client.resolve_dispute(&a, &false);
    assert!(!client.is_disputed(&a));
    assert_eq!(client.get_status(&a), TransactionStatus::Completed);
    assert_eq!(
        client.get_dispute_queue(&0, &10).0,
        ids(&env, &["C", "D", "B"])
    );

    // Upheld dispute: leaves the queue, transaction reverts to Failed.
    let d = String::from_str(&env, "D");
    client.resolve_dispute(&d, &true);
    assert_eq!(client.get_status(&d), TransactionStatus::Failed);
    assert_eq!(
        client.get_transaction(&d).failure_reason,
        String::from_str(&env, "dispute_upheld")
    );
    assert_eq!(client.get_dispute_queue(&0, &10).0, ids(&env, &["C", "B"]));
}

#[test]
fn pagination_walks_the_queue_with_stable_cursors() {
    let (env, client, relay) = setup();
    four_disputed(&env, &client, &relay);

    let (page1, next) = client.get_dispute_queue(&0, &2);
    assert_eq!(page1, ids(&env, &["C", "A"]));
    let cursor = next.expect("more pages");

    // Resolving the first item of the next page between calls neither skips
    // nor repeats anything.
    client.resolve_dispute(&String::from_str(&env, "D"), &false);
    let (page2, next) = client.get_dispute_queue(&cursor, &2);
    assert_eq!(page2, ids(&env, &["B"]));
    assert_eq!(next, None);
}

#[test]
fn exact_page_boundary_returns_no_next_cursor() {
    let (env, client, relay) = setup();
    four_disputed(&env, &client, &relay);
    let (page, next) = client.get_dispute_queue(&0, &4);
    assert_eq!(page.len(), 4);
    assert_eq!(next, None);
}

#[test]
fn re_disputed_transaction_rejoins_at_the_tail() {
    let (env, client, relay) = setup();
    four_disputed(&env, &client, &relay);
    let c = String::from_str(&env, "C");
    client.resolve_dispute(&c, &false);
    tick(&env);
    dispute(&env, &client, &relay, &c);
    assert_eq!(
        client.get_dispute_queue(&0, &10).0,
        ids(&env, &["A", "D", "B", "C"])
    );
}

#[test]
fn empty_queue_and_invalid_limits() {
    let (_env, client, _relay) = setup();
    let (page, next) = client.get_dispute_queue(&0, &10);
    assert_eq!(page.len(), 0);
    assert_eq!(next, None);

    for bad in [0, MAX_PAGE_LIMIT + 1] {
        assert_eq!(
            client.try_get_dispute_queue(&0, &bad),
            Err(Ok(ContractError::InvalidPageLimit))
        );
    }
}

#[test]
fn dispute_state_machine_guards() {
    let (env, client, relay) = setup();
    let a = completed(&env, &client, &relay, "A");

    // Only Completed transactions can be disputed.
    let account = String::from_str(&env, G_ADDR);
    let pending = client.register_callback(&CallbackPayload {
        transaction_id: String::from_str(&env, "P"),
        stellar_account: account.clone(),
        amount: 1,
        asset_code: String::from_str(&env, "USDC"),
        asset_issuer: account,
        idempotency_key: String::from_str(&env, "P"),
        anchor_transaction_id: String::from_str(&env, "anchor-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(&env, "pending"),
    });
    let reason = String::from_str(&env, "r");
    assert_eq!(
        client.try_dispute_transaction(&pending, &reason, &relay),
        Err(Ok(ContractError::InvalidStatusTransition))
    );

    // Resolving something that is not disputed.
    assert_eq!(
        client.try_resolve_dispute(&a, &true),
        Err(Ok(ContractError::NotDisputed))
    );

    dispute(&env, &client, &relay, &a);
    assert_eq!(
        client.try_dispute_transaction(&a, &reason, &relay),
        Err(Ok(ContractError::AlreadyDisputed))
    );

    // Upheld → Failed, which can no longer be disputed.
    client.resolve_dispute(&a, &true);
    assert_eq!(
        client.try_dispute_transaction(&a, &reason, &relay),
        Err(Ok(ContractError::InvalidStatusTransition))
    );
}

#[test]
fn stranger_cannot_raise_dispute() {
    let (env, client, relay) = setup();
    let a = completed(&env, &client, &relay, "A");
    let stranger = Address::generate(&env);
    assert_eq!(
        client.try_dispute_transaction(&a, &String::from_str(&env, "r"), &stranger),
        Err(Ok(ContractError::Unauthorised))
    );
    assert_eq!(client.get_dispute_queue(&0, &10).0.len(), 0);
}

#[test]
fn queue_is_bounded() {
    let (env, client, relay) = setup();
    for i in 0..MAX_OPEN_DISPUTES {
        let id = completed(&env, &client, &relay, &std::format!("q{i}"));
        dispute(&env, &client, &relay, &id);
    }
    let extra = completed(&env, &client, &relay, "overflow");
    assert_eq!(
        client.try_dispute_transaction(&extra, &String::from_str(&env, "r"), &relay),
        Err(Ok(ContractError::DisputeQueueFull))
    );

    // Resolving one frees a slot.
    client.resolve_dispute(&String::from_str(&env, "q0"), &false);
    dispute(&env, &client, &relay, &extra);
    let (page, next) = client.get_dispute_queue(&0, &MAX_PAGE_LIMIT);
    assert_eq!(page.get_unchecked(0), String::from_str(&env, "q1"));
    assert!(next.is_some());
}
