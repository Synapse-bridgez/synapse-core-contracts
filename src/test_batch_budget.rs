//! Tests for the `batch_register_callback` resource-budget check (#173).
//!
//! The budget is a second layer on top of the `MAX_BATCH_SIZE` count cap: a
//! batch within the count cap is still rejected, before any storage write,
//! when its conservative worst-case byte estimate exceeds either budget.

#![cfg(test)]

extern crate std;

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events},
    Address, Env, String, Symbol, TryFromVal, Vec,
};

use crate::types::{CallbackPayload, CallbackType, ContractError, MAX_BATCH_SIZE};
use crate::validation::{Validator, BATCH_EVENT_BUDGET_BYTES, BATCH_WRITE_BUDGET_BYTES};
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

/// A string of exactly `len` bytes: `prefix` padded with `x`.
fn padded(env: &Env, prefix: &str, len: usize) -> String {
    let mut s = std::string::String::from(prefix);
    while s.len() < len {
        s.push('x');
    }
    assert_eq!(s.len(), len, "prefix longer than requested length");
    String::from_str(env, &s)
}

/// A payload with every capped string field at its maximum length.
fn max_payload(env: &Env, tag: &str, idem_len: usize) -> CallbackPayload {
    let account = String::from_str(env, G_ADDR);
    CallbackPayload {
        transaction_id: padded(env, tag, 64),
        stellar_account: account.clone(),
        amount: 1_000,
        asset_code: String::from_str(env, "ABCDEFGHIJKL"),
        asset_issuer: account,
        idempotency_key: padded(env, tag, idem_len),
        anchor_transaction_id: padded(env, "anchor", 64),
        callback_type: CallbackType::Deposit,
        callback_status: padded(env, "status", 32),
    }
}

/// `MAX_BATCH_SIZE` max-length payloads whose write estimate equals
/// `BATCH_WRITE_BUDGET_BYTES + extra` exactly, by padding idempotency keys.
fn batch_at_write_budget(env: &Env, prefix: &str, extra: u32) -> Vec<CallbackPayload> {
    let n = MAX_BATCH_SIZE as usize;
    let base_idem = 8usize;
    let mut v = Vec::new(env);
    for i in 0..n {
        v.push_back(max_payload(
            env,
            &std::format!("{prefix}{i:02}-"),
            base_idem,
        ));
    }
    let base = Validator::estimate_batch_cost(&v).write_bytes;
    let slack = (BATCH_WRITE_BUDGET_BYTES + extra - base) as usize;

    let mut padded_v = Vec::new(env);
    for i in 0..n {
        let mut extra_len = slack / n;
        if i == n - 1 {
            extra_len += slack % n;
        }
        padded_v.push_back(max_payload(
            env,
            &std::format!("{prefix}{i:02}-"),
            base_idem + extra_len,
        ));
    }
    padded_v
}

fn assert_none_registered(client: &SynapseCoreContractClient, batch: &Vec<CallbackPayload>) {
    for p in batch.iter() {
        assert!(client.try_get_status(&p.transaction_id).is_err());
    }
}

#[test]
fn max_count_max_length_batch_fits_budget() {
    // The caps and budgets are sized so every legitimate batch passes: the
    // count cap alone, with every capped field at its limit, is under budget.
    let (env, client, relay) = setup();
    let mut v = Vec::new(&env);
    for i in 0..MAX_BATCH_SIZE {
        v.push_back(max_payload(&env, &std::format!("t{i:02}-"), 36));
    }
    let cost = Validator::estimate_batch_cost(&v);
    assert!(cost.write_bytes <= BATCH_WRITE_BUDGET_BYTES);
    assert!(cost.event_bytes <= BATCH_EVENT_BUDGET_BYTES);
    assert_eq!(client.batch_register_callback(&v, &relay), MAX_BATCH_SIZE);
}

#[test]
fn batch_exactly_at_write_budget_succeeds() {
    let (env, client, relay) = setup();
    let batch = batch_at_write_budget(&env, "a", 0);
    assert_eq!(
        Validator::estimate_batch_cost(&batch).write_bytes,
        BATCH_WRITE_BUDGET_BYTES
    );
    assert_eq!(
        client.batch_register_callback(&batch, &relay),
        MAX_BATCH_SIZE
    );
}

#[test]
fn batch_one_byte_over_write_budget_rejected_before_any_write() {
    let (env, client, relay) = setup();
    let batch = batch_at_write_budget(&env, "b", 1);
    assert_eq!(
        Validator::estimate_batch_cost(&batch).write_bytes,
        BATCH_WRITE_BUDGET_BYTES + 1
    );
    assert_eq!(
        client.try_batch_register_callback(&batch, &relay),
        Err(Ok(ContractError::BatchBudgetExceeded))
    );
    assert_none_registered(&client, &batch);
    // Nothing observable happened: the failed call emitted no events.
    assert_eq!(env.events().all().len(), 0);
}

#[test]
fn event_budget_is_enforced_independently_of_write_budget() {
    // Over-long anchor ids inflate the event estimate past its budget while
    // the write estimate stays well under. The budget check runs before
    // per-item validation, so it fires before the StringTooLong cap would.
    let (env, client, relay) = setup();
    let mut v = Vec::new(&env);
    for i in 0..MAX_BATCH_SIZE {
        let mut p = max_payload(&env, &std::format!("e{i:02}-"), 36);
        p.anchor_transaction_id = padded(&env, "anchor", 200);
        v.push_back(p);
    }
    let cost = Validator::estimate_batch_cost(&v);
    assert!(cost.write_bytes <= BATCH_WRITE_BUDGET_BYTES);
    assert!(cost.event_bytes > BATCH_EVENT_BUDGET_BYTES);
    assert_eq!(
        client.try_batch_register_callback(&v, &relay),
        Err(Ok(ContractError::BatchBudgetExceeded))
    );
    assert_none_registered(&client, &v);
}

#[test]
fn single_huge_item_rejected_by_budget_not_count() {
    // One item is well within the count cap but alone exceeds the budget:
    // the rejection is the distinguishable budget error, not InvalidBatchSize.
    let (env, client, relay) = setup();
    let mut v = Vec::new(&env);
    v.push_back(max_payload(
        &env,
        "huge-",
        BATCH_WRITE_BUDGET_BYTES as usize,
    ));
    assert_eq!(
        client.try_batch_register_callback(&v, &relay),
        Err(Ok(ContractError::BatchBudgetExceeded))
    );
    assert_none_registered(&client, &v);
}

#[test]
fn count_cap_still_checked_first() {
    let (env, client, relay) = setup();
    let mut v = Vec::new(&env);
    for i in 0..=MAX_BATCH_SIZE {
        v.push_back(max_payload(&env, &std::format!("c{i:02}-"), 16));
    }
    assert_eq!(
        client.try_batch_register_callback(&v, &relay),
        Err(Ok(ContractError::InvalidBatchSize))
    );
}

#[test]
fn estimate_grows_with_every_weighted_field() {
    let env = Env::default();
    let one = |p: CallbackPayload| {
        let mut v = Vec::new(&env);
        v.push_back(p);
        Validator::estimate_batch_cost(&v)
    };
    let base = one(max_payload(&env, "w-", 16));

    let mut p = max_payload(&env, "w-", 16);
    p.transaction_id = padded(&env, "w-", 65);
    let c = one(p);
    // tx_id: key + record (write), per-item event + first/last in summary.
    assert_eq!(c.write_bytes, base.write_bytes + 2);
    assert_eq!(c.event_bytes, base.event_bytes + 2);

    let mut p = max_payload(&env, "w-", 16);
    p.idempotency_key = padded(&env, "w-", 17);
    let c = one(p);
    assert_eq!(c.write_bytes, base.write_bytes + 1);
    assert_eq!(c.event_bytes, base.event_bytes);
}

#[test]
fn batch_emits_per_item_events_then_one_summary_last() {
    let (env, client, relay) = setup();
    let mut v = Vec::new(&env);
    for i in 0..3 {
        v.push_back(max_payload(&env, &std::format!("o{i:02}-"), 16));
    }
    client.batch_register_callback(&v, &relay);

    let events = env.events().all();
    assert_eq!(events.len(), 4);
    for (i, (_, topics, _)) in events.iter().enumerate() {
        let name = Symbol::try_from_val(&env, &topics.get_unchecked(1)).unwrap();
        let want = if i < 3 {
            symbol_short!("reg")
        } else {
            symbol_short!("batch")
        };
        assert_eq!(name, want);
    }
}

#[test]
fn batch_rejects_replayed_and_in_batch_duplicate_idempotency_keys() {
    let (env, client, relay) = setup();
    let mut first = Vec::new(&env);
    first.push_back(max_payload(&env, "d0-", 16));
    client.batch_register_callback(&first, &relay);

    // Same idempotency key as an already-registered item, fresh tx id.
    let mut replayed = max_payload(&env, "d1-", 16);
    replayed.idempotency_key = first.get_unchecked(0).idempotency_key;
    let mut v = Vec::new(&env);
    v.push_back(replayed);
    assert_eq!(
        client.try_batch_register_callback(&v, &relay),
        Err(Ok(ContractError::DuplicateRequest))
    );

    // Two items sharing an idempotency key within one batch.
    let a = max_payload(&env, "d2-", 16);
    let mut b = max_payload(&env, "d3-", 16);
    b.idempotency_key = a.idempotency_key.clone();
    let mut v = Vec::new(&env);
    v.push_back(a);
    v.push_back(b);
    assert_eq!(
        client.try_batch_register_callback(&v, &relay),
        Err(Ok(ContractError::DuplicateRequest))
    );
    assert_none_registered(&client, &v);
}

#[test]
fn batch_rejects_non_relay_caller() {
    let (env, client, _relay) = setup();
    let stranger = Address::generate(&env);
    let mut v = Vec::new(&env);
    v.push_back(max_payload(&env, "n0-", 16));
    assert_eq!(
        client.try_batch_register_callback(&v, &stranger),
        Err(Ok(ContractError::NotRelaySigner))
    );
}
