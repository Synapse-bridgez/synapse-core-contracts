//! # Ledger-I/O resource budgets (#116, #123, #125)
//!
//! Pins the ledger footprint of the hot and security-critical entry points,
//! so an accidental extra storage read or write fails CI instead of silently
//! raising every caller's fee.
//!
//! ## What is measured
//!
//! After each invocation, `env.cost_estimate().resources()` reports the
//! ledger entries it read and wrote and their byte sizes. These counts come
//! from the storage footprint, not from CPU metering, so they are exact and
//! deterministic in the native test build. CPU instructions are printed for
//! trend-tracking only: native tests do not meter contract code, so they
//! undercount (see `bench_resource_release_wasm` and COST_MODEL.md §12).
//!
//! Every write counts one entry per key, including the host's auth-nonce
//! entry for each `require_auth` (1 per invocation here). `R` excludes keys
//! that are also written.
//!
//! ## Updating a budget
//!
//! A failing budget means the entry point now touches more ledger state.
//! If that is intended, update the row in [`BUDGETS`] **and** COST_MODEL.md
//! §12 in the same PR, and say why in the PR description. Lowering a budget
//! after an optimisation is always welcome.
//!
//! Run `cargo test bench_resource -- --nocapture` to print the table.

#![cfg(test)]

extern crate std;

use std::println;

use soroban_sdk::{testutils::Address as _, Address, Env, String};

use crate::types::{CallbackPayload, CallbackType};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

/// `(scenario, max read entries, max write entries, max write bytes)`.
const BUDGETS: &[(&str, u32, u32, u32)] = &[
    ("register_callback", 3, 5, 1132),
    ("register_callback_replay", 4, 1, 72),
    ("batch_register_callback_x5", 3, 17, 4876),
    ("start_processing", 3, 8, 1976),
    ("start_processing_last_in_bucket", 3, 6, 1152),
    ("complete_transaction", 4, 8, 2000),
    ("fail_transaction", 3, 8, 1976),
    ("cancel_transaction", 3, 8, 1984),
    ("retry_transaction", 3, 6, 1144),
    ("get_transaction", 2, 0, 0),
    ("propose_admin", 3, 2, 200),
    ("accept_admin", 2, 3, 196),
];

struct Measured {
    name: &'static str,
    read_entries: u32,
    write_entries: u32,
    write_bytes: u32,
    instructions: i64,
    /// Estimated fee in stroops, excluding temporary-entry rent (dominated
    /// by the host's auth-nonce entry, which no contract change affects).
    fee: i64,
}

fn measure(env: &Env, name: &'static str) -> Measured {
    let r = env.cost_estimate().resources();
    let f = env.cost_estimate().fee();
    Measured {
        name,
        read_entries: r.read_entries,
        write_entries: r.write_entries,
        write_bytes: r.write_bytes,
        instructions: r.instructions,
        fee: f.total - f.temporary_entry_rent,
    }
}

fn payload(env: &Env, id: &str) -> CallbackPayload {
    let account = String::from_str(
        env,
        "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ",
    );
    CallbackPayload {
        transaction_id: String::from_str(env, id),
        stellar_account: account.clone(),
        amount: 1_000,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: account,
        idempotency_key: String::from_str(env, id),
        anchor_transaction_id: String::from_str(env, "anchor-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    }
}

fn setup(wasm: Option<&[u8]>) -> (Env, SynapseCoreContractClient<'static>, Address, Address) {
    let env = Env::default();
    let id = match wasm {
        Some(code) => env.register(code, ()),
        None => env.register(SynapseCoreContract, ()),
    };
    let client = SynapseCoreContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    (env, client, admin, relay)
}

/// Run every scenario on a fresh contract and return the measurements.
/// `wasm` runs them against a compiled contract instead of the native build.
fn run_scenarios(wasm: Option<&[u8]>) -> std::vec::Vec<Measured> {
    let mut out = std::vec::Vec::new();
    let hash = |env: &Env| String::from_str(env, "stellar-tx-hash");
    let reason = |env: &Env| String::from_str(env, "reason");

    // Ingestion.
    let (env, client, _admin, relay) = setup(wasm);
    client.register_callback(&payload(&env, "tx-0"));
    let tx = client.register_callback(&payload(&env, "tx-1"));
    out.push(measure(&env, "register_callback"));
    client.register_callback(&payload(&env, "tx-1"));
    out.push(measure(&env, "register_callback_replay"));
    let mut batch = soroban_sdk::Vec::new(&env);
    for id in ["b-1", "b-2", "b-3", "b-4", "b-5"] {
        batch.push_back(payload(&env, id));
    }
    client.batch_register_callback(&batch, &relay);
    out.push(measure(&env, "batch_register_callback_x5"));

    // Transitions. `tx-0` is first in the Pending bucket, so moving it
    // swaps the bucket's last entry into its slot (the common FIFO case).
    let first = String::from_str(&env, "tx-0");
    client.start_processing(&first, &relay);
    out.push(measure(&env, "start_processing"));
    // Moving tx-0 swapped b-5 into slot 0, so b-4 is now last: no swap.
    let last = String::from_str(&env, "b-4");
    client.start_processing(&last, &relay);
    out.push(measure(&env, "start_processing_last_in_bucket"));
    client.complete_transaction(&first, &hash(&env), &relay);
    out.push(measure(&env, "complete_transaction"));
    client.fail_transaction(&tx, &reason(&env), &relay);
    out.push(measure(&env, "fail_transaction"));
    client.cancel_transaction(&String::from_str(&env, "b-1"), &reason(&env), &relay);
    out.push(measure(&env, "cancel_transaction"));
    client.retry_transaction(&tx, &relay);
    out.push(measure(&env, "retry_transaction"));
    client.get_transaction(&tx);
    out.push(measure(&env, "get_transaction"));

    // Two-step admin transfer (#125).
    let (env, client, _admin, _relay) = setup(wasm);
    let nominee = Address::generate(&env);
    client.propose_admin(&nominee);
    out.push(measure(&env, "propose_admin"));
    client.accept_admin(&nominee);
    out.push(measure(&env, "accept_admin"));

    out
}

#[test]
fn bench_resource_budgets() {
    let measured = run_scenarios(None);
    let mut failures = std::vec::Vec::new();
    for m in &measured {
        println!(
            "[resources] {:<32} R={:>2} W={:>2} write_bytes={:>5} insns(native)={:>7}",
            m.name, m.read_entries, m.write_entries, m.write_bytes, m.instructions
        );
        let (_, max_r, max_w, max_wb) = BUDGETS
            .iter()
            .find(|b| b.0 == m.name)
            .unwrap_or_else(|| panic!("no budget row for scenario `{}`", m.name));
        if m.read_entries > *max_r || m.write_entries > *max_w || m.write_bytes > *max_wb {
            failures.push(std::format!(
                "{}: R={} W={} write_bytes={} exceeds budget R<={} W<={} write_bytes<={}",
                m.name,
                m.read_entries,
                m.write_entries,
                m.write_bytes,
                max_r,
                max_w,
                max_wb
            ));
        }
    }
    assert_eq!(
        measured.len(),
        BUDGETS.len(),
        "every BUDGETS row needs a scenario"
    );
    assert!(
        failures.is_empty(),
        "ledger-I/O budget exceeded (see bench_resources.rs docs):\n  {}",
        failures.join("\n  ")
    );
}

/// Opt-in: `SYNAPSE_BENCH_WASM=<path to release .wasm>` prints the same
/// scenarios metered against the compiled contract, where CPU instructions
/// are real. Read bytes then include the contract code entry. Not asserted:
/// the numbers depend on the exact toolchain that built the WASM.
#[test]
fn bench_resource_release_wasm() {
    let Ok(path) = std::env::var("SYNAPSE_BENCH_WASM") else {
        return;
    };
    let code = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    for m in run_scenarios(Some(&code)) {
        println!(
            "[resources:wasm] {:<32} R={:>2} W={:>2} write_bytes={:>5} insns={:>8} fee={:>7}",
            m.name, m.read_entries, m.write_entries, m.write_bytes, m.instructions, m.fee
        );
    }
}

/// The budget check must actually trip: a scenario over any limit fails.
#[test]
fn bench_resource_budget_detects_regression() {
    let over = Measured {
        name: "register_callback",
        read_entries: 4,
        write_entries: 5,
        write_bytes: 1000,
        instructions: 0,
        fee: 0,
    };
    let (_, max_r, max_w, max_wb) = BUDGETS[0];
    assert_eq!(BUDGETS[0].0, over.name);
    assert!(over.read_entries > max_r || over.write_entries > max_w || over.write_bytes > max_wb);
}

// ─── #173 batch resource budget ──────────────────────────────────────────────

/// Payload with every string field at `len` bytes (asset code capped at 12,
/// callback status at 32; the strkeys are fixed-length).
fn payload_sized(env: &Env, n: usize, len: usize) -> CallbackPayload {
    let mut p = payload(env, "x");
    let text = |c: char, l: usize| {
        let mut s = std::string::String::new();
        s.push_str(&std::format!("{n}-"));
        while s.len() < l.max(3) {
            s.push(c);
        }
        String::from_str(env, &s)
    };
    p.transaction_id = text('t', len.min(64));
    p.idempotency_key = text('k', len.min(64));
    p.anchor_transaction_id = text('a', len.min(64));
    p.callback_status = text('s', len.min(32));
    p.asset_code = String::from_str(env, &"U".repeat(len.clamp(1, 12)));
    p
}

/// For every batch size and field-length mix, the pre-write estimate is at
/// least the footprint the batch actually produced, so an accepted batch can
/// never hit a network limit mid-execution.
#[test]
fn batch_cost_estimate_is_conservative() {
    for len in [1usize, 16, 40, 64] {
        for n in 1..=crate::types::MAX_BATCH_SIZE {
            let (env, client, _admin, relay) = setup(None);
            let mut batch = soroban_sdk::Vec::new(&env);
            let mut est = crate::validation::BatchCost::new(0);
            for i in 0..n {
                let p = payload_sized(&env, i as usize, len);
                est.add(&p);
                batch.push_back(p);
            }
            client.batch_register_callback(&batch, &relay);
            let r = env.cost_estimate().resources();
            assert!(
                est.fits(),
                "n={n} len={len}: estimate must accept a batch that succeeded"
            );
            assert!(
                r.write_entries <= est.write_entries
                    && r.write_bytes <= est.write_bytes
                    && r.contract_events_size_bytes <= est.event_bytes,
                "n={n} len={len}: estimate {est:?} below measured W={} wb={} ev={}",
                r.write_entries,
                r.write_bytes,
                r.contract_events_size_bytes
            );
        }
    }
}

/// Boundary through the entry point. With an N-of-M relay set, every
/// consumed co-signer approval is one more ledger write, so a full batch of
/// `MAX_BATCH_SIZE` fits at exactly 25 writes with 2 approvals (threshold 3)
/// and is rejected, before any write, with 3 approvals (threshold 4).
#[test]
fn batch_budget_boundary_with_quorum_approvals() {
    for (threshold, expect_ok) in [(3u32, true), (4u32, false)] {
        let (env, client, _admin, relay) = setup(None);
        let cosigners: std::vec::Vec<Address> = (0..3).map(|_| Address::generate(&env)).collect();
        for s in &cosigners {
            client.add_relay_signer(s);
        }
        client.set_relay_threshold(&threshold);
        for s in cosigners.iter().take(threshold as usize - 1) {
            client.approve_relay_call(s);
        }
        let mut batch = soroban_sdk::Vec::new(&env);
        for i in 0..crate::types::MAX_BATCH_SIZE {
            batch.push_back(payload_sized(&env, i as usize, 8));
        }
        let first_id = batch.get_unchecked(0).transaction_id;
        let result = client.try_batch_register_callback(&batch, &relay);
        if expect_ok {
            assert_eq!(result, Ok(Ok(crate::types::MAX_BATCH_SIZE)));
            assert_eq!(env.cost_estimate().resources().write_entries, 25);
        } else {
            assert_eq!(
                result,
                Err(Ok(crate::types::ContractError::BatchResourceBudgetExceeded))
            );
            assert!(
                client.try_get_transaction(&first_id).is_err(),
                "no partial writes"
            );
        }
    }
}

/// One item over the count cap is still the plain size error.
#[test]
fn batch_over_count_cap_is_invalid_batch_size() {
    let (env, client, _admin, relay) = setup(None);
    let mut batch = soroban_sdk::Vec::new(&env);
    for i in 0..=crate::types::MAX_BATCH_SIZE {
        batch.push_back(payload_sized(&env, i as usize, 8));
    }
    assert_eq!(
        client.try_batch_register_callback(&batch, &relay),
        Err(Ok(crate::types::ContractError::InvalidBatchSize))
    );
}

/// The event-byte limit is enforced as well. With max-length payloads it
/// would bind at the 13th item, but the write-entry limit (and so
/// `MAX_BATCH_SIZE`) binds first, which is why a full batch always fits.
#[test]
fn batch_cost_enforces_event_byte_limit() {
    let env = Env::default();
    let mut cost = crate::validation::BatchCost::new(0);
    let mut added = 0u32;
    while cost.event_bytes <= crate::validation::TX_MAX_EVENTS_BYTES {
        cost.add(&payload_sized(&env, added as usize, 64));
        added += 1;
    }
    assert!(!cost.fits());
    assert_eq!(added, 13, "event budget binds at the 13th max-length item");
    assert!(
        cost.write_entries > crate::validation::TX_MAX_WRITE_ENTRIES,
        "at MAX_BATCH_SIZE the write-entry limit binds first ({added} items)"
    );
    let mut small = crate::validation::BatchCost::new(0);
    for i in 0..crate::types::MAX_BATCH_SIZE {
        small.add(&payload_sized(&env, i as usize, 64));
    }
    assert!(small.fits(), "a full batch of max-length payloads must fit");
    assert!(small.event_bytes < crate::validation::TX_MAX_EVENTS_BYTES);
}
