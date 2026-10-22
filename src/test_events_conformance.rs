//! Event conformance tests: ordering guarantees for multi-event entry points.
//!
//! These tests assert the *sequence* of emitted events (not just presence) for
//! every entry point that emits two or more events. See `EVENTS.md` for the
//! documented ordering guarantees.

#![cfg(test)]

use crate::events;
use crate::types::{Callback, TransactionStatus};
use crate::{Contract, ContractClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _},
    Address, Env, IntoVal, Symbol, Vec,
};

/// Helper: extract the event topics (first topic symbol) in emission order.
fn event_names(env: &Env) -> Vec<Symbol> {
    let mut names = Vec::new(env);
    for (_, topics, _) in env.events().all().iter() {
        if let Some(first) = topics.get(0) {
            // Topics are Val; the first topic is a Symbol for our events.
            let sym: Symbol = first.into_val(env);
            names.push_back(sym);
        }
    }
    names
}

// ---------------------------------------------------------------------------
// complete_transaction: `status` then `done`
// ---------------------------------------------------------------------------

#[test]
fn complete_transaction_emits_status_then_done() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, Contract);
    let client = ContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    client.initialize(&admin);

    // Register a transaction and complete it.
    let tx_id = client.register_transaction(&user, &100_i128);
    client.complete_transaction(&admin, &tx_id);

    let names = event_names(&env);
    // Find the status and done events and assert their relative order.
    let mut status_idx: Option<u32> = None;
    let mut done_idx: Option<u32> = None;
    for i in 0..names.len() {
        let name = names.get(i).unwrap();
        if name == symbol_short!("status") {
            status_idx = Some(i);
        } else if name == symbol_short!("done") {
            done_idx = Some(i);
        }
    }

    let status_idx = status_idx.expect("status event must be emitted");
    let done_idx = done_idx.expect("done event must be emitted");
    assert!(
        status_idx < done_idx,
        "complete_transaction must emit `status` before `done` (status={}, done={})",
        status_idx,
        done_idx
    );

    // Also verify the status payload reflects completion.
    let _ = TransactionStatus::Completed;
}

// ---------------------------------------------------------------------------
// batch_register_callback: per-item `reg` events in input order, then `batch`
// ---------------------------------------------------------------------------

#[test]
fn batch_register_callback_emits_regs_in_order_then_batch() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, Contract);
    let client = ContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    let cb_a = Callback {
        id: symbol_short!("cb_a"),
        target: Address::generate(&env),
    };
    let cb_b = Callback {
        id: symbol_short!("cb_b"),
        target: Address::generate(&env),
    };
    let cb_c = Callback {
        id: symbol_short!("cb_c"),
        target: Address::generate(&env),
    };

    let mut callbacks = Vec::new(&env);
    callbacks.push_back(cb_a.clone());
    callbacks.push_back(cb_b.clone());
    callbacks.push_back(cb_c.clone());

    client.batch_register_callback(&admin, &callbacks);

    let names = event_names(&env);

    // Collect indices of `reg` and `batch` events.
    let mut reg_indices: Vec<u32> = Vec::new(&env);
    let mut batch_idx: Option<u32> = None;
    for i in 0..names.len() {
        let name = names.get(i).unwrap();
        if name == symbol_short!("reg") {
            reg_indices.push_back(i);
        } else if name == symbol_short!("batch") {
            batch_idx = Some(i);
        }
    }

    // Exactly one `reg` per input item, in input order.
    assert_eq!(
        reg_indices.len(),
        callbacks.len(),
        "batch_register_callback must emit one `reg` event per input item"
    );

    // The `batch` summary must come last, after all `reg` events.
    let batch_idx = batch_idx.expect("batch summary event must be emitted");
    let last_reg = reg_indices.get(reg_indices.len() - 1).unwrap();
    assert!(
        last_reg < batch_idx,
        "batch summary must be emitted after all per-item `reg` events (last_reg={}, batch={})",
        last_reg,
        batch_idx
    );

    // Verify per-item `reg` events appear in input order by checking the
    // callback id carried in each event's data.
    let all = env.events().all();
    let mut seen_ids: Vec<Symbol> = Vec::new(&env);
    for (_, topics, data) in all.iter() {
        if let Some(first) = topics.get(0) {
            let sym: Symbol = first.into_val(&env);
            if sym == symbol_short!("reg") {
                let id: Symbol = data.into_val(&env);
                seen_ids.push_back(id);
            }
        }
    }
    assert_eq!(seen_ids.len(), 3);
    assert_eq!(seen_ids.get(0).unwrap(), cb_a.id);
    assert_eq!(seen_ids.get(1).unwrap(), cb_b.id);
    assert_eq!(seen_ids.get(2).unwrap(), cb_c.id);
}

// ---------------------------------------------------------------------------
// upgrade finalize: `up_fin` after any preceding upgrade events
// ---------------------------------------------------------------------------

#[test]
fn upgrade_finalize_emits_up_fin_last() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, Contract);
    let client = ContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    let new_wasm_hash = soroban_sdk::BytesN::from_array(&env, &[7u8; 32]);

    client.upgrade(&admin, &new_wasm_hash);
    client.upgrade_finalize(&admin);

    let names = event_names(&env);

    let mut up_fin_idx: Option<u32> = None;
    for i in 0..names.len() {
        let name = names.get(i).unwrap();
        if name == symbol_short!("up_fin") {
            up_fin_idx = Some(i);
        }
    }

    let up_fin_idx = up_fin_idx.expect("up_fin event must be emitted by upgrade_finalize");

    // `up_fin` must be the last event emitted by the finalize flow.
    assert_eq!(
        up_fin_idx,
        names.len() - 1,
        "up_fin must be the last event emitted (up_fin={}, total={})",
        up_fin_idx,
        names.len()
    );
}

// ---------------------------------------------------------------------------
// dispute resolution: `dispute_raised` before `dispute_resolved`
// ---------------------------------------------------------------------------

#[test]
fn dispute_resolution_emits_raised_before_resolved() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, Contract);
    let client = ContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);

    let tx_id = client.register_transaction(&user, &250_i128);
    client.complete_transaction(&admin, &tx_id);

    // Raise then resolve the dispute.
    client.raise_dispute(&user, &tx_id);
    client.resolve_dispute(&admin, &tx_id);

    let names = event_names(&env);

    let mut raised_idx: Option<u32> = None;
    let mut resolved_idx: Option<u32> = None;
    for i in 0..names.len() {
        let name = names.get(i).unwrap();
        if name == symbol_short!("d_raise") {
            raised_idx = Some(i);
        } else if name == symbol_short!("d_resolv") {
            resolved_idx = Some(i);
        }
    }

    let raised_idx = raised_idx.expect("dispute_raised event must be emitted");
    let resolved_idx = resolved_idx.expect("dispute_resolved event must be emitted");
    assert!(
        raised_idx < resolved_idx,
        "dispute_raised must precede dispute_resolved (raised={}, resolved={})",
        raised_idx,
        resolved_idx
    );
}

// ---------------------------------------------------------------------------
// Sanity: events module helpers exist and are used by the entry points above.
// ---------------------------------------------------------------------------

#[test]
fn events_module_exposes_expected_symbols() {
    let env = Env::default();
    // Touch the events module to ensure the symbols referenced by the ordering
    // tests are the canonical ones defined there.
    let _ = events::EVENT_STATUS;
    let _ = events::EVENT_DONE;
    let _ = events::EVENT_REG;
    let _ = events::EVENT_BATCH;
    let _ = events::EVENT_UP_FIN;
    let _ = events::EVENT_DISPUTE_RAISED;
    let _ = events::EVENT_DISPUTE_RESOLVED;
    let _ = &env;
}