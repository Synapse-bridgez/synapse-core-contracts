// tools/export-test-vectors/src/main.rs
//
// Generates `test-vectors.json` — a language-agnostic, independently
// replayable set of test scenarios for the synapse-core-contract.
//
// ## Purpose (THREAT_MODEL.md §10.5)
//
// An external auditor can use these vectors to cross-check on-chain contract
// behavior against documented expectations using their own tooling, without
// needing to run or trust this repo's Rust test suite.
//
// ## What each vector contains
//
// * `description`      — Human-readable intent of the scenario.
// * `category`         — Functional area (lifecycle, fee, treasury, …).
// * `entry_point`      — Contract function under test.
// * `inputs`           — Parameters passed to the entry point.
// * `pre_state`        — On-chain state required before the call.
// * `expected_result`  — "ok" | "err:<ErrorVariant>".
// * `expected_events`  — Events that must (or must not) be emitted.
// * `expected_state`   — Key on-chain state changes after the call.
// * `notes`            — Cross-references to THREAT_MODEL.md findings, §8
//                        accepted risks, or invariants from §5.
//
// ## Usage
//
//   cd tools/export-test-vectors
//   cargo run -- --pretty > ../../test-vectors.json
//
// The format is documented in docs/test-vectors.md.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

// ─── Data types ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct TestVectorFile {
    schema_version: u32,
    contract_version: String,
    description: String,
    reference_docs: Vec<String>,
    vectors: Vec<TestVector>,
}

#[derive(Debug, Serialize)]
struct TestVector {
    id: String,
    description: String,
    category: String,
    entry_point: String,
    inputs: BTreeMap<String, Value>,
    pre_state: BTreeMap<String, Value>,
    expected_result: String,
    expected_events: Vec<EventExpectation>,
    expected_state: BTreeMap<String, Value>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct EventExpectation {
    /// Topics as strings, e.g. ["synapse", "reg"].
    topics: Vec<String>,
    /// true = event must be present; false = event must NOT be present.
    present: bool,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn s(v: &str) -> String {
    v.to_string()
}

fn event_expect(topics: &[&str], present: bool) -> EventExpectation {
    EventExpectation {
        topics: topics.iter().map(|t| t.to_string()).collect(),
        present,
    }
}

fn synapse_event(name: &str) -> EventExpectation {
    event_expect(&["synapse", name], true)
}

fn no_event(name: &str) -> EventExpectation {
    event_expect(&["synapse", name], false)
}

fn inputs(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn state(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn v_str(s: &str) -> Value {
    Value::String(s.to_string())
}

fn v_i128(n: i128) -> Value {
    // JSON has no native i128; represent as string to avoid precision loss.
    Value::String(n.to_string())
}

fn v_u32(n: u32) -> Value {
    serde_json::json!(n)
}

fn v_bool(b: bool) -> Value {
    Value::Bool(b)
}

fn v_null() -> Value {
    Value::Null
}

fn notes(items: &[&str]) -> Vec<String> {
    items.iter().map(|n| n.to_string()).collect()
}

/// Canonical checksum-valid SEP-23 G-address used across all vectors.
const G_ADDR: &str = "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ";

// ─── Vector definitions ───────────────────────────────────────────────────────

fn vectors() -> Vec<TestVector> {
    vec![
        // ── initialize() ─────────────────────────────────────────────────────

        TestVector {
            id: s("init-001"),
            description: s("initialize() with valid admin and relay — succeeds and sets state"),
            category: s("initialisation"),
            entry_point: s("initialize"),
            inputs: inputs(&[
                ("admin",           v_str("<admin_address>")),
                ("relay_signer",    v_str("<relay_address>")),
            ]),
            pre_state: state(&[
                ("initialized", v_bool(false)),
            ]),
            expected_result: s("ok"),
            expected_events: vec![synapse_event("init")],
            expected_state: state(&[
                ("initialized",    v_bool(true)),
                ("admin",          v_str("<admin_address>")),
                ("relay_signer",   v_str("<relay_address>")),
                ("paused",         v_bool(false)),
                ("schema_version", v_u32(1)),
            ]),
            notes: notes(&[
                "THREAT_MODEL §4.1: double-init guard enforced",
                "I-08: contract must be initialised before state-mutating calls",
            ]),
        },

        TestVector {
            id: s("init-002"),
            description: s("initialize() called twice — returns AlreadyInitialised"),
            category: s("initialisation"),
            entry_point: s("initialize"),
            inputs: inputs(&[
                ("admin",           v_str("<admin_address>")),
                ("relay_signer",    v_str("<relay_address>")),
            ]),
            pre_state: state(&[("initialized", v_bool(true))]),
            expected_result: s("err:AlreadyInitialised"),
            expected_events: vec![no_event("init")],
            expected_state: state(&[]),
            notes: notes(&["ContractError::AlreadyInitialised = 1"]),
        },

        // ── register_callback() ───────────────────────────────────────────────

        TestVector {
            id: s("reg-001"),
            description: s("register_callback() with valid payload — creates Pending transaction"),
            category: s("lifecycle"),
            entry_point: s("register_callback"),
            inputs: inputs(&[
                ("transaction_id",        v_str("tx-uuid-001")),
                ("stellar_account",       v_str(G_ADDR)),
                ("amount",                v_i128(1_000_000)),
                ("asset_code",            v_str("USDC")),
                ("asset_issuer",          v_str(G_ADDR)),
                ("idempotency_key",       v_str("idem-001")),
                ("anchor_transaction_id", v_str("anchor-001")),
                ("callback_type",         v_str("Deposit")),
                ("callback_status",       v_str("pending_external")),
            ]),
            pre_state: state(&[
                ("initialized",        v_bool(true)),
                ("paused",             v_bool(false)),
                ("transaction_exists", v_bool(false)),
                ("idempotency_known",  v_bool(false)),
            ]),
            expected_result: s("ok:\"tx-uuid-001\""),
            expected_events: vec![synapse_event("reg")],
            expected_state: state(&[
                ("transaction.status",          v_str("Pending")),
                ("transaction.stellar_tx_hash", v_str("")),
                ("transaction.failure_reason",  v_str("")),
                ("idempotency_key_recorded",    v_bool(true)),
            ]),
            notes: notes(&[
                "I-01: transaction created in Pending state",
                "I-10: stellar_tx_hash empty until Completed",
                "I-11: failure_reason empty until Failed",
            ]),
        },

        TestVector {
            id: s("reg-002"),
            description: s("register_callback() duplicate idempotency_key within TTL — idempotent return, no write"),
            category: s("idempotency"),
            entry_point: s("register_callback"),
            inputs: inputs(&[
                ("transaction_id",  v_str("tx-uuid-001")),
                ("idempotency_key", v_str("idem-001")),
            ]),
            pre_state: state(&[("idempotency_known", v_bool(true))]),
            expected_result: s("ok:\"tx-uuid-001\""),
            expected_events: vec![no_event("reg")],
            expected_state: state(&[]),
            notes: notes(&[
                "R-04: idempotency window finite — within window, deduplication works",
                "No second write occurs",
            ]),
        },

        TestVector {
            id: s("reg-003"),
            description: s("register_callback() same transaction_id after idempotency TTL expiry — DuplicateRequest (F-07)"),
            category: s("idempotency"),
            entry_point: s("register_callback"),
            inputs: inputs(&[
                ("transaction_id",  v_str("tx-uuid-001")),
                ("idempotency_key", v_str("idem-fresh-new-key")),
            ]),
            pre_state: state(&[
                ("idempotency_known",  v_bool(false)),
                ("transaction_exists", v_bool(true)),
            ]),
            expected_result: s("err:DuplicateRequest"),
            expected_events: vec![no_event("reg")],
            expected_state: state(&[]),
            notes: notes(&[
                "F-07 fixed: persistent transaction_id guard, independent of idempotency TTL",
                "R-04 compensating control",
                "ContractError::DuplicateRequest = 40",
            ]),
        },

        TestVector {
            id: s("reg-004"),
            description: s("register_callback() while paused — ContractPaused"),
            category: s("pause"),
            entry_point: s("register_callback"),
            inputs: inputs(&[("transaction_id", v_str("tx-any"))]),
            pre_state: state(&[("paused", v_bool(true))]),
            expected_result: s("err:ContractPaused"),
            expected_events: vec![],
            expected_state: state(&[]),
            notes: notes(&[
                "ContractError::ContractPaused = 12",
                "R-01 compensating control: pause halts new ingestion",
            ]),
        },

        TestVector {
            id: s("reg-005"),
            description: s("register_callback() with invalid stellar_account — InvalidStellarAccount"),
            category: s("validation"),
            entry_point: s("register_callback"),
            inputs: inputs(&[("stellar_account", v_str("GSHORT"))]),
            pre_state: state(&[("initialized", v_bool(true)), ("paused", v_bool(false))]),
            expected_result: s("err:InvalidStellarAccount"),
            expected_events: vec![],
            expected_state: state(&[]),
            notes: notes(&[
                "F-01 fixed: full SEP-23 base32 + CRC16 validation",
                "ContractError::InvalidStellarAccount = 20",
            ]),
        },

        TestVector {
            id: s("reg-006"),
            description: s("register_callback() with zero amount — InvalidAmount"),
            category: s("validation"),
            entry_point: s("register_callback"),
            inputs: inputs(&[("amount", v_i128(0))]),
            pre_state: state(&[("initialized", v_bool(true)), ("paused", v_bool(false))]),
            expected_result: s("err:InvalidAmount"),
            expected_events: vec![],
            expected_state: state(&[]),
            notes: notes(&["ContractError::InvalidAmount = 21"]),
        },

        TestVector {
            id: s("reg-007"),
            description: s("register_callback() transaction_id over 64-byte cap — StringTooLong"),
            category: s("validation"),
            entry_point: s("register_callback"),
            inputs: inputs(&[("transaction_id", v_str(&"t".repeat(65)))]),
            pre_state: state(&[("initialized", v_bool(true)), ("paused", v_bool(false))]),
            expected_result: s("err:StringTooLong"),
            expected_events: vec![],
            expected_state: state(&[]),
            notes: notes(&[
                "MAX_TX_ID_LEN = 64",
                "ContractError::StringTooLong = 25",
            ]),
        },

        // ── start_processing() ────────────────────────────────────────────────

        TestVector {
            id: s("proc-001"),
            description: s("start_processing() on Pending transaction — transitions to Processing"),
            category: s("lifecycle"),
            entry_point: s("start_processing"),
            inputs: inputs(&[
                ("tx_id",  v_str("tx-uuid-001")),
                ("caller", v_str("<relay_or_admin_address>")),
            ]),
            pre_state: state(&[("transaction.status", v_str("Pending"))]),
            expected_result: s("ok"),
            expected_events: vec![synapse_event("status")],
            expected_state: state(&[("transaction.status", v_str("Processing"))]),
            notes: notes(&["I-02: only Pending→Processing is valid here"]),
        },

        TestVector {
            id: s("proc-002"),
            description: s("start_processing() on Completed transaction — InvalidStatusTransition"),
            category: s("lifecycle"),
            entry_point: s("start_processing"),
            inputs: inputs(&[
                ("tx_id",  v_str("tx-uuid-001")),
                ("caller", v_str("<relay_or_admin_address>")),
            ]),
            pre_state: state(&[("transaction.status", v_str("Completed"))]),
            expected_result: s("err:InvalidStatusTransition"),
            expected_events: vec![],
            expected_state: state(&[("transaction.status", v_str("Completed"))]),
            notes: notes(&[
                "I-05: Completed is terminal",
                "ContractError::InvalidStatusTransition = 31",
            ]),
        },

        TestVector {
            id: s("proc-003"),
            description: s("start_processing() by non-admin/non-relay — Unauthorised"),
            category: s("auth"),
            entry_point: s("start_processing"),
            inputs: inputs(&[
                ("tx_id",  v_str("tx-uuid-001")),
                ("caller", v_str("<attacker_address>")),
            ]),
            pre_state: state(&[
                ("transaction.status", v_str("Pending")),
                ("caller_is_admin",    v_bool(false)),
                ("caller_is_relay",    v_bool(false)),
            ]),
            expected_result: s("err:Unauthorised"),
            expected_events: vec![],
            expected_state: state(&[("transaction.status", v_str("Pending"))]),
            notes: notes(&["ContractError::Unauthorised = 10"]),
        },

        // ── complete_transaction() ────────────────────────────────────────────

        TestVector {
            id: s("comp-001"),
            description: s("complete_transaction() on Processing — Completed, fee accrued (100 bps on 1_000_000)"),
            category: s("lifecycle"),
            entry_point: s("complete_transaction"),
            inputs: inputs(&[
                ("tx_id",           v_str("tx-uuid-001")),
                ("stellar_tx_hash", v_str("abc123stellarhash")),
                ("caller",          v_str("<relay_or_admin_address>")),
            ]),
            pre_state: state(&[
                ("transaction.status", v_str("Processing")),
                ("transaction.amount", v_i128(1_000_000)),
                ("param.base_fee_bps", v_i128(100)),
                ("treasury_balance",   v_i128(0)),
            ]),
            expected_result: s("ok"),
            expected_events: vec![
                synapse_event("status"),
                synapse_event("done"),
                synapse_event("fee"),
            ],
            expected_state: state(&[
                ("transaction.status",          v_str("Completed")),
                ("transaction.stellar_tx_hash", v_str("abc123stellarhash")),
                ("treasury_balance",            v_i128(10_000)),
            ]),
            notes: notes(&[
                "I-03: Processing→Completed is the only valid transition",
                "I-10: stellar_tx_hash set at Completed",
                "fee = floor(1_000_000 * 100 / 10_000) = 10_000 stroops",
                "#141 fee accrual",
                "Events emitted in order: status, done, fee",
            ]),
        },

        TestVector {
            id: s("comp-002"),
            description: s("complete_transaction() with base_fee_bps=0 — Completed, no fee event"),
            category: s("fee"),
            entry_point: s("complete_transaction"),
            inputs: inputs(&[
                ("tx_id",           v_str("tx-uuid-001")),
                ("stellar_tx_hash", v_str("abc123stellarhash")),
                ("caller",          v_str("<relay_or_admin_address>")),
            ]),
            pre_state: state(&[
                ("transaction.status", v_str("Processing")),
                ("param.base_fee_bps", v_i128(0)),
                ("treasury_balance",   v_i128(0)),
            ]),
            expected_result: s("ok"),
            expected_events: vec![
                synapse_event("status"),
                synapse_event("done"),
                no_event("fee"),
            ],
            expected_state: state(&[
                ("transaction.status", v_str("Completed")),
                ("treasury_balance",   v_i128(0)),
            ]),
            notes: notes(&[
                "With base_fee_bps=0 (or unset), no fee is accrued and no fee event is emitted",
                "Events emitted: exactly 2 (status, done)",
            ]),
        },

        TestVector {
            id: s("comp-003"),
            description: s("complete_transaction() on Pending (skipping Processing) — InvalidStatusTransition"),
            category: s("lifecycle"),
            entry_point: s("complete_transaction"),
            inputs: inputs(&[
                ("tx_id",           v_str("tx-uuid-001")),
                ("stellar_tx_hash", v_str("abc123stellarhash")),
                ("caller",          v_str("<relay_or_admin_address>")),
            ]),
            pre_state: state(&[("transaction.status", v_str("Pending"))]),
            expected_result: s("err:InvalidStatusTransition"),
            expected_events: vec![],
            expected_state: state(&[("transaction.status", v_str("Pending"))]),
            notes: notes(&[
                "State machine strictly enforced: Pending→Completed is not a valid transition",
            ]),
        },

        // ── fail_transaction() ────────────────────────────────────────────────

        TestVector {
            id: s("fail-001"),
            description: s("fail_transaction() on Pending — Failed, no fee accrued"),
            category: s("lifecycle"),
            entry_point: s("fail_transaction"),
            inputs: inputs(&[
                ("tx_id",  v_str("tx-uuid-001")),
                ("reason", v_str("horizon_timeout")),
                ("caller", v_str("<relay_or_admin_address>")),
            ]),
            pre_state: state(&[
                ("transaction.status", v_str("Pending")),
                ("param.base_fee_bps", v_i128(100)),
                ("treasury_balance",   v_i128(0)),
            ]),
            expected_result: s("ok"),
            expected_events: vec![synapse_event("status"), synapse_event("fail")],
            expected_state: state(&[
                ("transaction.status",         v_str("Failed")),
                ("transaction.failure_reason", v_str("horizon_timeout")),
                ("treasury_balance",           v_i128(0)),
            ]),
            notes: notes(&[
                "I-04: Pending→Failed is valid",
                "I-11: failure_reason set at Failed",
                "No fee accrual on Failed — fees only on Completed",
            ]),
        },

        TestVector {
            id: s("fail-002"),
            description: s("fail_transaction() on Completed — InvalidStatusTransition"),
            category: s("lifecycle"),
            entry_point: s("fail_transaction"),
            inputs: inputs(&[
                ("tx_id",  v_str("tx-uuid-001")),
                ("reason", v_str("late-fail")),
                ("caller", v_str("<relay_or_admin_address>")),
            ]),
            pre_state: state(&[("transaction.status", v_str("Completed"))]),
            expected_result: s("err:InvalidStatusTransition"),
            expected_events: vec![],
            expected_state: state(&[("transaction.status", v_str("Completed"))]),
            notes: notes(&["I-05: Completed is terminal — cannot be failed after completion"]),
        },

        // ── set_param("base_fee_bps") ────────────────────────────────────────

        TestVector {
            id: s("fee-001"),
            description: s("set_param(base_fee_bps, 200) by admin — updates fee rate, emits param event"),
            category: s("fee"),
            entry_point: s("set_param"),
            inputs: inputs(&[("name", v_str("base_fee_bps")), ("value", v_i128(200))]),
            pre_state: state(&[("param.base_fee_bps", v_i128(100)), ("caller_is_admin", v_bool(true))]),
            expected_result: s("ok"),
            expected_events: vec![synapse_event("param")],
            expected_state: state(&[("param.base_fee_bps", v_i128(200))]),
            notes: notes(&["#141: fee rate is the base_fee_bps registry param; applies to future completions only"]),
        },

        TestVector {
            id: s("fee-002"),
            description: s("set_param(base_fee_bps, 10_001) — InvalidParamValue"),
            category: s("fee"),
            entry_point: s("set_param"),
            inputs: inputs(&[("name", v_str("base_fee_bps")), ("value", v_i128(10_001))]),
            pre_state: state(&[("param.base_fee_bps", v_i128(100)), ("caller_is_admin", v_bool(true))]),
            expected_result: s("err:InvalidParamValue"),
            expected_events: vec![no_event("param")],
            expected_state: state(&[("param.base_fee_bps", v_i128(100))]),
            notes: notes(&["ContractError::InvalidParamValue = 71", "Valid range 0..=10_000"]),
        },

        TestVector {
            id: s("fee-003"),
            description: s("set_param(base_fee_bps) by non-admin — auth error"),
            category: s("fee"),
            entry_point: s("set_param"),
            inputs: inputs(&[("name", v_str("base_fee_bps")), ("value", v_i128(200))]),
            pre_state: state(&[("caller_is_admin", v_bool(false))]),
            expected_result: s("err:auth_error"),
            expected_events: vec![],
            expected_state: state(&[]),
            notes: notes(&["R-02: all admin operations require admin auth"]),
        },

        TestVector {
            id: s("fee-004"),
            description: s("complete_transaction() with amount = i128::MAX at 1 bps — exact fee, no overflow"),
            category: s("fee"),
            entry_point: s("complete_transaction"),
            inputs: inputs(&[
                ("tx_id",           v_str("tx-uuid-max")),
                ("stellar_tx_hash", v_str("abc123stellarhash")),
                ("caller",          v_str("<relay_or_admin_address>")),
            ]),
            pre_state: state(&[
                ("transaction.status", v_str("Processing")),
                ("transaction.amount", v_i128(i128::MAX)),
                ("param.base_fee_bps", v_i128(1)),
                ("treasury_balance",   v_i128(0)),
            ]),
            expected_result: s("ok"),
            expected_events: vec![
                synapse_event("status"),
                synapse_event("done"),
                synapse_event("fee"),
            ],
            expected_state: state(&[
                ("transaction.status", v_str("Completed")),
                ("treasury_balance",   v_i128(i128::MAX / 10_000)),
            ]),
            notes: notes(&[
                "fee = floor(amount * base_fee_bps / 10_000), computed without an intermediate overflow",
                "#141: checked arithmetic; see docs/adr/0008-fee-accrual-and-treasury-withdrawal.md",
            ]),
        },

        TestVector {
            id: s("fee-005"),
            description: s("complete_transaction() whose fee would overflow the treasury — ArithmeticOverflow, call rolled back"),
            category: s("fee"),
            entry_point: s("complete_transaction"),
            inputs: inputs(&[
                ("tx_id",           v_str("tx-uuid-002")),
                ("stellar_tx_hash", v_str("abc123stellarhash")),
                ("caller",          v_str("<relay_or_admin_address>")),
            ]),
            pre_state: state(&[
                ("transaction.status", v_str("Processing")),
                ("transaction.amount", v_i128(1)),
                ("param.base_fee_bps", v_i128(10_000)),
                ("treasury_balance",   v_i128(i128::MAX)),
            ]),
            expected_result: s("err:ArithmeticOverflow"),
            expected_events: vec![],
            expected_state: state(&[
                ("transaction.status", v_str("Processing")),
                ("treasury_balance",   v_i128(i128::MAX)),
            ]),
            notes: notes(&[
                "ContractError::ArithmeticOverflow = 116",
                "#141: treasury accounting must never wrap",
            ]),
        },

        // ── propose_withdrawal() ──────────────────────────────────────────────

        TestVector {
            id: s("wdraw-001"),
            description: s("propose_withdrawal() by admin — creates PendingWithdrawal, emits wprop"),
            category: s("treasury"),
            entry_point: s("propose_withdrawal"),
            inputs: inputs(&[
                ("amount",      v_i128(5_000)),
                ("destination", v_str("<destination_address>")),
            ]),
            pre_state: state(&[
                ("caller_is_admin",      v_bool(true)),
                ("treasury_balance",     v_i128(10_000)),
                ("param.treasury_epoch_cap",    v_i128(1_000_000)),
                ("param.treasury_epoch_length", v_i128(1_000)),
                ("pending_withdrawal",   v_null()),
            ]),
            expected_result: s("ok"),
            expected_events: vec![synapse_event("wprop")],
            expected_state: state(&[
                ("pending_withdrawal.amount",      v_i128(5_000)),
                ("pending_withdrawal.destination", v_str("<destination_address>")),
            ]),
            notes: notes(&["#142 step 1 of 2: admin proposes, relay must co-authorize"]),
        },

        TestVector {
            id: s("wdraw-002"),
            description: s("propose_withdrawal() amount > treasury balance — InsufficientTreasuryBalance"),
            category: s("treasury"),
            entry_point: s("propose_withdrawal"),
            inputs: inputs(&[("amount", v_i128(10_001))]),
            pre_state: state(&[
                ("caller_is_admin",     v_bool(true)),
                ("treasury_balance",    v_i128(10_000)),
                ("param.treasury_epoch_cap",    v_i128(1_000_000)),
                ("param.treasury_epoch_length", v_i128(1_000)),
            ]),
            expected_result: s("err:InsufficientTreasuryBalance"),
            expected_events: vec![],
            expected_state: state(&[]),
            notes: notes(&["ContractError::InsufficientTreasuryBalance = 114"]),
        },

        TestVector {
            id: s("wdraw-003"),
            description: s("propose_withdrawal() when treasury not configured — TreasuryNotConfigured"),
            category: s("treasury"),
            entry_point: s("propose_withdrawal"),
            inputs: inputs(&[("amount", v_i128(1))]),
            pre_state: state(&[
                ("caller_is_admin",     v_bool(true)),
                ("param.treasury_epoch_cap",    v_null()),
            ]),
            expected_result: s("err:TreasuryNotConfigured"),
            expected_events: vec![],
            expected_state: state(&[]),
            notes: notes(&["ContractError::TreasuryNotConfigured = 115"]),
        },

        // ── authorize_withdrawal() ────────────────────────────────────────────

        TestVector {
            id: s("wdraw-004"),
            description: s("authorize_withdrawal() by relay signer — executes withdrawal, emits wexec"),
            category: s("treasury"),
            entry_point: s("authorize_withdrawal"),
            inputs: inputs(&[
                ("caller",      v_str("<relay_address>")),
                ("amount",      v_i128(5_000)),
                ("destination", v_str("<destination_address>")),
            ]),
            pre_state: state(&[
                ("caller_is_relay",           v_bool(true)),
                ("pending_withdrawal.amount", v_i128(5_000)),
                ("treasury_balance",          v_i128(10_000)),
                ("epoch_withdrawn",           v_i128(0)),
                ("param.treasury_epoch_cap",  v_i128(1_000_000)),
            ]),
            expected_result: s("ok"),
            expected_events: vec![synapse_event("wexec")],
            expected_state: state(&[
                ("treasury_balance",   v_i128(5_000)),
                ("pending_withdrawal", v_null()),
                ("epoch_withdrawn",    v_i128(5_000)),
            ]),
            notes: notes(&[
                "#142 step 2 of 2: relay co-authorizes and executes",
                "Two-party authorization: admin proposes, relay authorizes",
                "Treasury balance reduced by withdrawal amount",
            ]),
        },

        TestVector {
            id: s("wdraw-005"),
            description: s("authorize_withdrawal() by admin (not relay) — WithdrawalNotRelaySigner"),
            category: s("treasury"),
            entry_point: s("authorize_withdrawal"),
            inputs: inputs(&[
                ("caller",      v_str("<admin_address>")),
                ("amount",      v_i128(5_000)),
                ("destination", v_str("<destination_address>")),
            ]),
            pre_state: state(&[
                ("caller_is_relay",           v_bool(false)),
                ("caller_is_admin",           v_bool(true)),
                ("pending_withdrawal.amount", v_i128(5_000)),
            ]),
            expected_result: s("err:WithdrawalNotRelaySigner"),
            expected_events: vec![],
            expected_state: state(&[
                ("pending_withdrawal_still_present", v_bool(true)),
                ("treasury_balance_unchanged",        v_bool(true)),
            ]),
            notes: notes(&[
                "ContractError::WithdrawalNotRelaySigner = 113",
                "#142: admin cannot unilaterally execute its own proposal",
                "R-02 compensating control: two-party authorization prevents single-key drainage",
            ]),
        },

        TestVector {
            id: s("wdraw-006"),
            description: s("authorize_withdrawal() exceeds per-epoch cap — WithdrawalCapExceeded"),
            category: s("treasury"),
            entry_point: s("authorize_withdrawal"),
            inputs: inputs(&[
                ("caller",      v_str("<relay_address>")),
                ("amount",      v_i128(1)),
                ("destination", v_str("<destination_address>")),
            ]),
            pre_state: state(&[
                ("caller_is_relay",           v_bool(true)),
                ("pending_withdrawal.amount", v_i128(1)),
                ("epoch_withdrawn",           v_i128(1_000_000)),
                ("param.treasury_epoch_cap",  v_i128(1_000_000)),
            ]),
            expected_result: s("err:WithdrawalCapExceeded"),
            expected_events: vec![],
            expected_state: state(&[]),
            notes: notes(&[
                "ContractError::WithdrawalCapExceeded = 111",
                "epoch_withdrawn + amount > epoch_cap triggers rejection",
            ]),
        },

        TestVector {
            id: s("wdraw-007"),
            description: s("authorize_withdrawal() after epoch reset — cap resets, withdrawal succeeds"),
            category: s("treasury"),
            entry_point: s("authorize_withdrawal"),
            inputs: inputs(&[
                ("caller",      v_str("<relay_address>")),
                ("amount",      v_i128(1)),
                ("destination", v_str("<destination_address>")),
            ]),
            pre_state: state(&[
                ("caller_is_relay",           v_bool(true)),
                ("pending_withdrawal.amount", v_i128(1)),
                ("epoch_withdrawn",           v_i128(1_000_000)),
                ("param.treasury_epoch_cap",  v_i128(1_000_000)),
                ("ledger_sequence",           v_str(">= epoch_start + treasury_epoch_length")),
            ]),
            expected_result: s("ok"),
            expected_events: vec![synapse_event("wexec")],
            expected_state: state(&[("epoch_withdrawn", v_i128(1))]),
            notes: notes(&[
                "Epoch resets automatically when now >= epoch_start + treasury_epoch_length",
                "epoch_withdrawn counter starts at 0 in new epoch",
            ]),
        },

        TestVector {
            id: s("wdraw-008"),
            description: s("set_param(treasury_epoch_length, 0) — InvalidParamValue"),
            category: s("treasury"),
            entry_point: s("set_param"),
            inputs: inputs(&[("name", v_str("treasury_epoch_length")), ("value", v_i128(0))]),
            pre_state: state(&[("caller_is_admin", v_bool(true))]),
            expected_result: s("err:InvalidParamValue"),
            expected_events: vec![no_event("param")],
            expected_state: state(&[]),
            notes: notes(&[
                "ContractError::InvalidParamValue = 71",
                "A zero-length epoch would reset the cap on every call, defeating it",
            ]),
        },

        TestVector {
            id: s("wdraw-009"),
            description: s("authorize_withdrawal() naming a different amount/destination than the pending proposal — WithdrawalProposalMismatch"),
            category: s("treasury"),
            entry_point: s("authorize_withdrawal"),
            inputs: inputs(&[
                ("caller",      v_str("<relay_address>")),
                ("amount",      v_i128(500)),
                ("destination", v_str("<destination_address>")),
            ]),
            pre_state: state(&[
                ("caller_is_relay",                v_bool(true)),
                ("pending_withdrawal.amount",      v_i128(1_000)),
                ("pending_withdrawal.destination", v_str("<attacker_address>")),
                ("treasury_balance",               v_i128(5_000)),
            ]),
            expected_result: s("err:WithdrawalProposalMismatch"),
            expected_events: vec![no_event("wexec")],
            expected_state: state(&[
                ("treasury_balance",                v_i128(5_000)),
                ("pending_withdrawal_still_present", v_bool(true)),
            ]),
            notes: notes(&[
                "ContractError::WithdrawalProposalMismatch = 117",
                "R-06 control (d): the relay co-signs one specific (amount, destination); an admin-side proposal swap after relay review cannot execute",
            ]),
        },

        // ── pause() / unpause() ───────────────────────────────────────────────

        TestVector {
            id: s("pause-001"),
            description: s("pause() by admin — blocks register_callback, emits pause event"),
            category: s("pause"),
            entry_point: s("pause"),
            inputs: inputs(&[]),
            pre_state: state(&[("paused", v_bool(false)), ("caller_is_admin", v_bool(true))]),
            expected_result: s("ok"),
            expected_events: vec![synapse_event("pause")],
            expected_state: state(&[("paused", v_bool(true))]),
            notes: notes(&[
                "R-01 compensating control: pause halts new ingestion",
                "Status transitions intentionally still work while paused (draining in-flight work)",
            ]),
        },

        TestVector {
            id: s("pause-002"),
            description: s("unpause() by admin — resumes register_callback, emits pause event"),
            category: s("pause"),
            entry_point: s("unpause"),
            inputs: inputs(&[]),
            pre_state: state(&[("paused", v_bool(true)), ("caller_is_admin", v_bool(true))]),
            expected_result: s("ok"),
            expected_events: vec![synapse_event("pause")],
            expected_state: state(&[("paused", v_bool(false))]),
            notes: notes(&[]),
        },

        TestVector {
            id: s("pause-003"),
            description: s("pause() by non-admin — auth error, contract stays unpaused"),
            category: s("pause"),
            entry_point: s("pause"),
            inputs: inputs(&[]),
            pre_state: state(&[("paused", v_bool(false)), ("caller_is_admin", v_bool(false))]),
            expected_result: s("err:auth_error"),
            expected_events: vec![],
            expected_state: state(&[("paused", v_bool(false))]),
            notes: notes(&["R-02: pause gated by admin auth"]),
        },

        // ── propose_admin() / accept_admin() ──────────────────────────────────

        TestVector {
            id: s("admin-001"),
            description: s("propose_admin() — nominates new_admin without transferring yet"),
            category: s("admin"),
            entry_point: s("propose_admin"),
            inputs: inputs(&[("new_admin", v_str("<new_admin_address>"))]),
            pre_state: state(&[("caller_is_admin", v_bool(true))]),
            expected_result: s("ok"),
            expected_events: vec![synapse_event("propose")],
            expected_state: state(&[
                ("admin",         v_str("<old_admin_address>")),
                ("pending_admin", v_str("<new_admin_address>")),
            ]),
            notes: notes(&[
                "F-03 fixed: two-step transfer — propose alone does not transfer admin",
            ]),
        },

        TestVector {
            id: s("admin-002"),
            description: s("accept_admin() by nominee — completes transfer, emits admin event"),
            category: s("admin"),
            entry_point: s("accept_admin"),
            inputs: inputs(&[("caller", v_str("<new_admin_address>"))]),
            pre_state: state(&[("pending_admin", v_str("<new_admin_address>"))]),
            expected_result: s("ok"),
            expected_events: vec![synapse_event("admin")],
            expected_state: state(&[
                ("admin",         v_str("<new_admin_address>")),
                ("pending_admin", v_null()),
            ]),
            notes: notes(&[
                "R-02 compensating control (c): nominee must prove key control",
            ]),
        },

        // ── upgrade() ────────────────────────────────────────────────────────

        TestVector {
            id: s("upg-001"),
            description: s("upgrade() with wrong expected_schema_version — SchemaVersionMismatch, WASM unchanged"),
            category: s("upgrade"),
            entry_point: s("upgrade"),
            inputs: inputs(&[
                ("new_wasm_hash",           v_str("<32-byte WASM hash>")),
                ("expected_schema_version", v_u32(999)),
            ]),
            pre_state: state(&[
                ("schema_version",  v_u32(1)),
                ("caller_is_admin", v_bool(true)),
            ]),
            expected_result: s("err:SchemaVersionMismatch"),
            expected_events: vec![no_event("upgrade")],
            expected_state: state(&[("wasm_unchanged", v_bool(true))]),
            notes: notes(&[
                "F-04 fixed: schema version guard fires before update_current_contract_wasm",
                "R-05 compensating control (c)",
                "ContractError::SchemaVersionMismatch = 60",
            ]),
        },

        TestVector {
            id: s("upg-002"),
            description: s("upgrade() by non-admin — auth error, WASM unchanged"),
            category: s("upgrade"),
            entry_point: s("upgrade"),
            inputs: inputs(&[
                ("new_wasm_hash",           v_str("<32-byte WASM hash>")),
                ("expected_schema_version", v_u32(1)),
            ]),
            pre_state: state(&[
                ("schema_version",  v_u32(1)),
                ("caller_is_admin", v_bool(false)),
            ]),
            expected_result: s("err:auth_error"),
            expected_events: vec![no_event("upgrade")],
            expected_state: state(&[("wasm_unchanged", v_bool(true))]),
            notes: notes(&[
                "R-05 compensating control (a): upgrade requires admin auth",
                "R-02: admin operations are auth-gated",
            ]),
        },
    ]
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let pretty = std::env::args().any(|a| a == "--pretty");

    let file = TestVectorFile {
        schema_version: 1,
        contract_version: s("0.1.0"),
        description: s(
            "Language-agnostic test vectors for synapse-core-contract. \
             Each vector specifies entry-point inputs, required pre-state, expected result, \
             expected events, and expected post-state. An external auditor can replay these \
             against a deployed contract instance using any tooling that can invoke Soroban \
             contract methods and read ledger state/events.",
        ),
        reference_docs: vec![
            s("THREAT_MODEL.md"),
            s("EVENTS.md"),
            s("docs/adr/0001-relay-signer-trust-model.md"),
            s("docs/adr/0002-two-step-admin-transfer.md"),
            s("docs/adr/0003-upgrade-schema-version-guard.md"),
            s("docs/adr/0008-fee-accrual-and-treasury-withdrawal.md"),
            s("docs/test-vectors.md"),
        ],
        vectors: vectors(),
    };

    let output = if pretty {
        serde_json::to_string_pretty(&file).expect("serialization failed")
    } else {
        serde_json::to_string(&file).expect("serialization failed")
    };

    println!("{output}");
}
