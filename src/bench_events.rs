//! # Event-emission cost benchmarks
//!
//! This module measures the CPU-instruction and memory-byte cost of each
//! [`EventEmitter`] variant in isolation, using the Soroban test SDK's budget
//! API.
//!
//! ## Methodology
//!
//! Each benchmark uses a two-phase approach:
//!
//! 1. **Baseline** — call `env.as_contract(id, || {})` (empty closure) to
//!    measure the irreducible overhead of entering a contract context.  Budget
//!    tracker is reset via `reset_tracker()` immediately before.
//! 2. **Measured** — call `env.as_contract(id, || EventEmitter::*(…))` with
//!    the event publication included.  Budget tracker is reset again before.
//! 3. **Delta** — `measured − baseline` isolates the event-emission cost.
//!
//! Budget tracking is done via `env.cost_estimate().budget()`:
//! * `cpu_instruction_cost()` — cumulative CPU instructions since last reset.
//! * `memory_bytes_cost()` — cumulative heap bytes since last reset.
//!
//! ## Stability note
//!
//! The Soroban SDK docs note that CPU instructions and memory usage are
//! **underestimated in native Rust tests** compared to WASM execution.  The
//! numbers here reflect *relative* cost (which event type is cheapest) and
//! *directional* trends (total event cost vs. lifecycle cost) rather than
//! exact on-chain values.  WASM numbers will be proportionally higher but
//! directionally identical.
//!
//! ## Interpreting the output
//!
//! Each `#[test]` containing `bench_` in its name:
//! * asserts `cpu > 0` — confirms the emitter actually consumed budget.
//! * asserts `cpu <= SANITY_CPU_CEILING` — catches catastrophic regressions.
//! * prints a `[bench]`-prefixed summary line to stderr for CI grep-ability.
//!
//! ```text
//! [bench] init       cpu=12345   mem=456
//! ```
//!
//! ## Wave-7 cumulative cost
//!
//! `bench_wave7_cumulative_cost` sums all ten per-event deltas and asserts a
//! regression ceiling.  This is the canonical CI gate for event-cost tracking.
//! Update `CUMULATIVE_CPU_CEILING` / `CUMULATIVE_MEM_CEILING` in this file
//! **and** in `COST_MODEL.md` §9 whenever a payload legitimately grows.

#![cfg(test)]

extern crate std;

use std::eprintln;

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String};

use crate::events::EventEmitter;
use crate::types::TransactionStatus;
use crate::SynapseCoreContract;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Per-event sanity ceiling for CPU cost (native Rust instructions).
/// Conservatively high — fails only on genuine regressions.
const SANITY_CPU_CEILING: u64 = 500_000;

/// Per-event sanity ceiling for memory cost (native Rust bytes).
const SANITY_MEM_CEILING: u64 = 100_000;

/// Realistic Stellar G-address (checksum-valid, passes CRC16 validation).
const G_ADDRESS_STR: &str = "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ";

/// Typical transaction UUID.
const TX_ID_STR: &str = "550e8400-e29b-41d4-a716-446655440000";

/// Realistic Stellar transaction hash (64 hex chars).
const STELLAR_HASH_STR: &str = "d3b07384d113edec49eaa6238ad5ff00a975b0a5b2c6b2b13d7b6e6e10f7b3c1";

// ─── Result type ─────────────────────────────────────────────────────────────

/// Net budget cost attributable to a single `EventEmitter::*` call.
#[derive(Debug, Clone, Copy)]
struct BenchResult {
    /// CPU instructions (delta: measured minus baseline).
    cpu: u64,
    /// Memory bytes (delta: measured minus baseline).
    mem: u64,
}

// ─── Core measurement helper ──────────────────────────────────────────────────

/// Measure the net budget cost of `emit_fn()` versus an empty baseline.
///
/// `emit_fn` is called inside `env.as_contract(contract_id, …)` so that
/// `env.events().publish(…)` is legal (it requires a contract context).
/// The budget tracker is reset to zero before both the baseline and the
/// measured run.
fn measure_event<E>(env: &Env, contract_id: &Address, emit_fn: E) -> BenchResult
where
    E: FnOnce(),
{
    // Baseline: enter the contract context without doing anything.
    env.cost_estimate().budget().reset_tracker();
    env.as_contract(contract_id, || {});
    let base_cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let base_mem = env.cost_estimate().budget().memory_bytes_cost();

    // Measured: enter the same context and call the emitter.
    env.cost_estimate().budget().reset_tracker();
    env.as_contract(contract_id, emit_fn);
    let full_cpu = env.cost_estimate().budget().cpu_instruction_cost();
    let full_mem = env.cost_estimate().budget().memory_bytes_cost();

    BenchResult {
        cpu: full_cpu.saturating_sub(base_cpu),
        mem: full_mem.saturating_sub(base_mem),
    }
}

// ─── Per-event benchmarks ─────────────────────────────────────────────────────

/// Benchmark `EventEmitter::initialised` (topic: `init`).
///
/// Payload: 2 × `Address` + `u32` (ledger sequence).
#[test]
fn bench_event_init() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    env.cost_estimate().budget().reset_default();

    let admin = Address::generate(&env);
    let relay = Address::generate(&env);

    let result = measure_event(&env, &contract_id, || {
        EventEmitter::initialised(&env, &admin, &relay);
    });

    eprintln!(
        "[bench] init       cpu={:<8}  mem={:<8}",
        result.cpu, result.mem
    );
    assert!(
        result.cpu > 0,
        "init event consumed zero CPU — check harness"
    );
    assert!(
        result.cpu <= SANITY_CPU_CEILING,
        "init CPU {cpu} exceeds ceiling {SANITY_CPU_CEILING}",
        cpu = result.cpu
    );
    assert!(
        result.mem <= SANITY_MEM_CEILING,
        "init mem {mem} exceeds ceiling {SANITY_MEM_CEILING}",
        mem = result.mem
    );
}

/// Benchmark `EventEmitter::transaction_registered` (topic: `reg`).
///
/// Heaviest payload: 4 × `String` + `i128` + `u32`.
#[test]
fn bench_event_reg() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    env.cost_estimate().budget().reset_default();

    let tx = crate::types::Transaction {
        id: String::from_str(&env, TX_ID_STR),
        stellar_account: String::from_str(&env, G_ADDRESS_STR),
        amount: 10_000_000,
        asset_code: String::from_str(&env, "USDC"),
        asset_issuer: String::from_str(&env, G_ADDRESS_STR),
        status: TransactionStatus::Pending,
        created_at_ledger: 1,
        updated_at_ledger: 1,
        anchor_transaction_id: String::from_str(&env, "anchor-tx-abc123"),
        callback_type: crate::types::CallbackType::Deposit,
        callback_status: String::from_str(&env, "pending_external"),
        stellar_tx_hash: String::from_str(&env, ""),
        failure_reason: String::from_str(&env, ""),
        retry_count: 0,
        settled_amount: None,
    };

    let result = measure_event(&env, &contract_id, || {
        EventEmitter::transaction_registered(&env, &tx);
    });

    eprintln!(
        "[bench] reg        cpu={:<8}  mem={:<8}",
        result.cpu, result.mem
    );
    assert!(result.cpu > 0, "reg event consumed zero CPU");
    assert!(
        result.cpu <= SANITY_CPU_CEILING,
        "reg CPU {cpu} exceeds ceiling {SANITY_CPU_CEILING}",
        cpu = result.cpu
    );
    assert!(
        result.mem <= SANITY_MEM_CEILING,
        "reg mem {mem} exceeds ceiling {SANITY_MEM_CEILING}",
        mem = result.mem
    );
}

/// Benchmark `EventEmitter::status_changed` (topic: `status`).
///
/// Payload: `String` + 2 × `TransactionStatus` + `u32`.
#[test]
fn bench_event_status() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    env.cost_estimate().budget().reset_default();

    let tx_id = String::from_str(&env, TX_ID_STR);

    let result = measure_event(&env, &contract_id, || {
        EventEmitter::status_changed(
            &env,
            &tx_id,
            TransactionStatus::Pending,
            TransactionStatus::Processing,
        );
    });

    eprintln!(
        "[bench] status     cpu={:<8}  mem={:<8}",
        result.cpu, result.mem
    );
    assert!(result.cpu > 0, "status event consumed zero CPU");
    assert!(
        result.cpu <= SANITY_CPU_CEILING,
        "status CPU {cpu} exceeds ceiling",
        cpu = result.cpu
    );
    assert!(
        result.mem <= SANITY_MEM_CEILING,
        "status mem {mem} exceeds ceiling",
        mem = result.mem
    );
}

/// Benchmark `EventEmitter::transaction_completed` (topic: `done`).
///
/// Payload: 2 × `String` + `u32`.
#[test]
fn bench_event_done() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    env.cost_estimate().budget().reset_default();

    let tx_id = String::from_str(&env, TX_ID_STR);
    let hash = String::from_str(&env, STELLAR_HASH_STR);

    let result = measure_event(&env, &contract_id, || {
        EventEmitter::transaction_completed(&env, &tx_id, &hash);
    });

    eprintln!(
        "[bench] done       cpu={:<8}  mem={:<8}",
        result.cpu, result.mem
    );
    assert!(result.cpu > 0, "done event consumed zero CPU");
    assert!(
        result.cpu <= SANITY_CPU_CEILING,
        "done CPU {cpu} exceeds ceiling",
        cpu = result.cpu
    );
    assert!(
        result.mem <= SANITY_MEM_CEILING,
        "done mem {mem} exceeds ceiling",
        mem = result.mem
    );
}

/// Benchmark `EventEmitter::transaction_failed` (topic: `fail`).
///
/// Payload: 2 × `String` + `u32`.
#[test]
fn bench_event_fail() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    env.cost_estimate().budget().reset_default();

    let tx_id = String::from_str(&env, TX_ID_STR);
    let reason = String::from_str(&env, "horizon_timeout");

    let result = measure_event(&env, &contract_id, || {
        EventEmitter::transaction_failed(&env, &tx_id, &reason);
    });

    eprintln!(
        "[bench] fail       cpu={:<8}  mem={:<8}",
        result.cpu, result.mem
    );
    assert!(result.cpu > 0, "fail event consumed zero CPU");
    assert!(
        result.cpu <= SANITY_CPU_CEILING,
        "fail CPU {cpu} exceeds ceiling",
        cpu = result.cpu
    );
    assert!(
        result.mem <= SANITY_MEM_CEILING,
        "fail mem {mem} exceeds ceiling",
        mem = result.mem
    );
}

/// Benchmark `EventEmitter::admin_transfer_proposed` (topic: `propose`).
///
/// Payload: 2 × `Address` + `u32`.
#[test]
fn bench_event_propose() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    env.cost_estimate().budget().reset_default();

    let current = Address::generate(&env);
    let proposed = Address::generate(&env);

    let result = measure_event(&env, &contract_id, || {
        EventEmitter::admin_transfer_proposed(&env, &current, &proposed);
    });

    eprintln!(
        "[bench] propose    cpu={:<8}  mem={:<8}",
        result.cpu, result.mem
    );
    assert!(result.cpu > 0, "propose event consumed zero CPU");
    assert!(
        result.cpu <= SANITY_CPU_CEILING,
        "propose CPU {cpu} exceeds ceiling",
        cpu = result.cpu
    );
    assert!(
        result.mem <= SANITY_MEM_CEILING,
        "propose mem {mem} exceeds ceiling",
        mem = result.mem
    );
}

/// Benchmark `EventEmitter::admin_transferred` (topic: `admin`).
///
/// Payload: 2 × `Address` + `u32`.
#[test]
fn bench_event_admin() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    env.cost_estimate().budget().reset_default();

    let old_admin = Address::generate(&env);
    let new_admin = Address::generate(&env);

    let result = measure_event(&env, &contract_id, || {
        EventEmitter::admin_transferred(&env, &old_admin, &new_admin);
    });

    eprintln!(
        "[bench] admin      cpu={:<8}  mem={:<8}",
        result.cpu, result.mem
    );
    assert!(result.cpu > 0, "admin event consumed zero CPU");
    assert!(
        result.cpu <= SANITY_CPU_CEILING,
        "admin CPU {cpu} exceeds ceiling",
        cpu = result.cpu
    );
    assert!(
        result.mem <= SANITY_MEM_CEILING,
        "admin mem {mem} exceeds ceiling",
        mem = result.mem
    );
}

/// Benchmark `EventEmitter::relay_signer_rotated` (topic: `relay`).
///
/// Payload: 2 × `Address` + `u32`.
#[test]
fn bench_event_relay() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    env.cost_estimate().budget().reset_default();

    let old_relay = Address::generate(&env);
    let new_relay = Address::generate(&env);

    let result = measure_event(&env, &contract_id, || {
        EventEmitter::relay_signer_rotated(&env, &old_relay, &new_relay);
    });

    eprintln!(
        "[bench] relay      cpu={:<8}  mem={:<8}",
        result.cpu, result.mem
    );
    assert!(result.cpu > 0, "relay event consumed zero CPU");
    assert!(
        result.cpu <= SANITY_CPU_CEILING,
        "relay CPU {cpu} exceeds ceiling",
        cpu = result.cpu
    );
    assert!(
        result.mem <= SANITY_MEM_CEILING,
        "relay mem {mem} exceeds ceiling",
        mem = result.mem
    );
}

/// Benchmark `EventEmitter::pause_toggled` (topic: `pause`).
///
/// Payload: `bool` + `Address` + `u32`.
#[test]
fn bench_event_pause() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    env.cost_estimate().budget().reset_default();

    let admin = Address::generate(&env);

    let result = measure_event(&env, &contract_id, || {
        EventEmitter::pause_toggled(&env, true, &admin);
    });

    eprintln!(
        "[bench] pause      cpu={:<8}  mem={:<8}",
        result.cpu, result.mem
    );
    assert!(result.cpu > 0, "pause event consumed zero CPU");
    assert!(
        result.cpu <= SANITY_CPU_CEILING,
        "pause CPU {cpu} exceeds ceiling",
        cpu = result.cpu
    );
    assert!(
        result.mem <= SANITY_MEM_CEILING,
        "pause mem {mem} exceeds ceiling",
        mem = result.mem
    );
}

/// Benchmark `EventEmitter::contract_upgraded` (topic: `upgrade`).
///
/// Payload: `Address` + `BytesN<32>` + 2 × `u32`.
#[test]
fn bench_event_upgrade() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    env.cost_estimate().budget().reset_default();

    let admin = Address::generate(&env);
    let wasm_hash = BytesN::from_array(&env, &[0xabu8; 32]);

    let result = measure_event(&env, &contract_id, || {
        EventEmitter::contract_upgraded(&env, &admin, &wasm_hash, 1);
    });

    eprintln!(
        "[bench] upgrade    cpu={:<8}  mem={:<8}",
        result.cpu, result.mem
    );
    assert!(result.cpu > 0, "upgrade event consumed zero CPU");
    assert!(
        result.cpu <= SANITY_CPU_CEILING,
        "upgrade CPU {cpu} exceeds ceiling",
        cpu = result.cpu
    );
    assert!(
        result.mem <= SANITY_MEM_CEILING,
        "upgrade mem {mem} exceeds ceiling",
        mem = result.mem
    );
}

// ─── Cumulative / wave cost regression gate ───────────────────────────────────

/// Sum per-event costs for Wave 7 and assert a regression ceiling.
///
/// This is the **CI regression gate** for event-emission cost.  If either
/// assertion fires, update `CUMULATIVE_CPU_CEILING` / `CUMULATIVE_MEM_CEILING`
/// here **and** add a row to the `COST_MODEL.md` §9 ceiling history table.
///
/// Lifecycle events per happy-path transaction:
/// * `register_callback`:    1 × `reg`
/// * `start_processing`:     1 × `status`
/// * `complete_transaction`: 1 × `status` + 1 × `done`
///
/// Administrative events (one-time, amortised across many transactions):
/// `init`, `propose`, `admin`, `relay`, `pause`, `upgrade`.
// ── Regression gate ───────────────────────────────────────────────────────────
// Update these constants AND add a row to COST_MODEL.md §9 when changing.
//
// Ceiling history:
//   2026-Q3: CPU=5_000_000  MEM=1_000_000  (initial measurement, Wave 7)
const CUMULATIVE_CPU_CEILING: u64 = 5_000_000;
const CUMULATIVE_MEM_CEILING: u64 = 1_000_000;

#[test]
#[allow(clippy::too_many_lines)] // one straight-line measurement per event; splitting hides the sum
fn bench_wave7_cumulative_cost() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    env.cost_estimate().budget().reset_default();

    // ── Fixtures ──────────────────────────────────────────────────────────
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    let tx_id = String::from_str(&env, TX_ID_STR);
    let hash = String::from_str(&env, STELLAR_HASH_STR);
    let reason = String::from_str(&env, "horizon_timeout");
    let wasm_hash = BytesN::from_array(&env, &[0xabu8; 32]);
    let tx = crate::types::Transaction {
        id: tx_id.clone(),
        stellar_account: String::from_str(&env, G_ADDRESS_STR),
        amount: 10_000_000,
        asset_code: String::from_str(&env, "USDC"),
        asset_issuer: String::from_str(&env, G_ADDRESS_STR),
        status: TransactionStatus::Pending,
        created_at_ledger: 1,
        updated_at_ledger: 1,
        anchor_transaction_id: String::from_str(&env, "anchor-tx-abc123"),
        callback_type: crate::types::CallbackType::Deposit,
        callback_status: String::from_str(&env, "pending_external"),
        stellar_tx_hash: String::from_str(&env, ""),
        failure_reason: String::from_str(&env, ""),
        retry_count: 0,
        settled_amount: None,
    };

    // ── Measure ───────────────────────────────────────────────────────────
    let r_init = measure_event(&env, &contract_id, || {
        EventEmitter::initialised(&env, &admin, &relay);
    });
    let r_reg = measure_event(&env, &contract_id, || {
        EventEmitter::transaction_registered(&env, &tx);
    });
    let r_status = measure_event(&env, &contract_id, || {
        EventEmitter::status_changed(
            &env,
            &tx_id,
            TransactionStatus::Pending,
            TransactionStatus::Processing,
        );
    });
    let r_done = measure_event(&env, &contract_id, || {
        EventEmitter::transaction_completed(&env, &tx_id, &hash);
    });
    let r_fail = measure_event(&env, &contract_id, || {
        EventEmitter::transaction_failed(&env, &tx_id, &reason);
    });
    let r_propose = measure_event(&env, &contract_id, || {
        EventEmitter::admin_transfer_proposed(&env, &admin, &relay);
    });
    let r_admin_t = measure_event(&env, &contract_id, || {
        EventEmitter::admin_transferred(&env, &admin, &relay);
    });
    let r_relay = measure_event(&env, &contract_id, || {
        EventEmitter::relay_signer_rotated(&env, &admin, &relay);
    });
    let r_pause = measure_event(&env, &contract_id, || {
        EventEmitter::pause_toggled(&env, true, &admin);
    });
    let r_upgrade = measure_event(&env, &contract_id, || {
        EventEmitter::contract_upgraded(&env, &admin, &wasm_hash, 1);
    });

    // ── Sums ──────────────────────────────────────────────────────────────
    // Per-lifecycle happy-path: reg + 2×status + done
    let lifecycle_cpu = r_reg.cpu + 2 * r_status.cpu + r_done.cpu;
    let lifecycle_mem = r_reg.mem + 2 * r_status.mem + r_done.mem;

    let total_cpu = r_init.cpu
        + r_reg.cpu
        + r_status.cpu
        + r_done.cpu
        + r_fail.cpu
        + r_propose.cpu
        + r_admin_t.cpu
        + r_relay.cpu
        + r_pause.cpu
        + r_upgrade.cpu;
    let total_mem = r_init.mem
        + r_reg.mem
        + r_status.mem
        + r_done.mem
        + r_fail.mem
        + r_propose.mem
        + r_admin_t.mem
        + r_relay.mem
        + r_pause.mem
        + r_upgrade.mem;

    // ── Output ────────────────────────────────────────────────────────────
    eprintln!("[bench] ─── per-event breakdown ──────────────────────────────");
    eprintln!(
        "[bench] init       cpu={:<8}  mem={:<8}",
        r_init.cpu, r_init.mem
    );
    eprintln!(
        "[bench] reg        cpu={:<8}  mem={:<8}",
        r_reg.cpu, r_reg.mem
    );
    eprintln!(
        "[bench] status     cpu={:<8}  mem={:<8}",
        r_status.cpu, r_status.mem
    );
    eprintln!(
        "[bench] done       cpu={:<8}  mem={:<8}",
        r_done.cpu, r_done.mem
    );
    eprintln!(
        "[bench] fail       cpu={:<8}  mem={:<8}",
        r_fail.cpu, r_fail.mem
    );
    eprintln!(
        "[bench] propose    cpu={:<8}  mem={:<8}",
        r_propose.cpu, r_propose.mem
    );
    eprintln!(
        "[bench] admin      cpu={:<8}  mem={:<8}",
        r_admin_t.cpu, r_admin_t.mem
    );
    eprintln!(
        "[bench] relay      cpu={:<8}  mem={:<8}",
        r_relay.cpu, r_relay.mem
    );
    eprintln!(
        "[bench] pause      cpu={:<8}  mem={:<8}",
        r_pause.cpu, r_pause.mem
    );
    eprintln!(
        "[bench] upgrade    cpu={:<8}  mem={:<8}",
        r_upgrade.cpu, r_upgrade.mem
    );
    eprintln!("[bench] ─── lifecycle totals ──────────────────────────────────");
    eprintln!(
        "[bench] lifecycle (reg+2xstatus+done)  cpu={lifecycle_cpu:<8}  mem={lifecycle_mem:<8}"
    );
    eprintln!("[bench] all 10 events                  cpu={total_cpu:<8}  mem={total_mem:<8}");

    // ── Regression gate (see CUMULATIVE_*_CEILING above) ─────────────────
    assert!(
        total_cpu <= CUMULATIVE_CPU_CEILING,
        "cumulative event CPU {total_cpu} exceeds ceiling {CUMULATIVE_CPU_CEILING}; \
         update COST_MODEL.md §9 if intentional"
    );
    assert!(
        total_mem <= CUMULATIVE_MEM_CEILING,
        "cumulative event mem {total_mem} exceeds ceiling {CUMULATIVE_MEM_CEILING}; \
         update COST_MODEL.md §9 if intentional"
    );
    assert!(lifecycle_cpu > 0, "lifecycle event cost computed as zero");
}
