//! # Event-payload snapshot regression tests (#133)
//!
//! Pins the exact topics and payload bytes every entry point emits, as
//! committed fixtures under `fixtures/event_snapshots/`. Any drift in a topic,
//! a field name, a field's type, field order, a value's encoding, or
//! multi-event emission order fails `cargo test` with a line diff.
//!
//! ## Why not `test_snapshots/`?
//!
//! The soroban-sdk writes `test_snapshots/**/*.json` automatically at the end
//! of every test and **overwrites** them unconditionally; the directory is
//! git-ignored in this repo. That mechanism can never fail on a change, so it
//! cannot be a regression gate. These fixtures are compared, never silently
//! rewritten.
//!
//! ## Updating a snapshot (deliberately)
//!
//! ```bash
//! SYNAPSE_UPDATE_EVENT_SNAPSHOTS=1 cargo test event_snapshot
//! git diff fixtures/event_snapshots/   # review, then commit with the change
//! ```
//!
//! A missing fixture is a failure too (not an implicit "accept"), and CI
//! additionally runs `git diff --exit-code fixtures/event_snapshots/` after
//! the test step, so an update can only land as a reviewed diff.
//!
//! ## Fixture format
//!
//! Each emitted event is three lines — readable topics, readable payload,
//! and the raw XDR (`topics-hex|data-hex`) — see
//! [`crate::test_support::render_last_events`]. Everything is deterministic:
//! addresses come from the test host's counter-based generator and the
//! ledger sequence is pinned to [`SNAPSHOT_LEDGER`].
//!
//! ## Catalogue coverage
//!
//! [`every_catalogued_event_has_a_snapshot_or_a_tracked_gap`] cross-checks
//! the EVENTS.md §2 status table against the topics covered by fixtures.
//! Gaps are explicit entries in [`KNOWN_GAPS`] / [`UNCATALOGUED_TOPICS`]; the
//! test fails if a new gap appears *or* if a tracked gap is closed without
//! updating the list.

#![cfg(test)]

extern crate std;

use std::{
    collections::BTreeSet,
    format,
    string::{String as StdString, ToString},
    vec::Vec,
};

use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, BytesN, Env, String,
};

use crate::events::EventEmitter;
use crate::test_support::{line_diff, render_last_events};
use crate::types::{CallbackPayload, CallbackType, SlashEvidence};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

/// Ledger sequence pinned for every snapshotted invocation.
const SNAPSHOT_LEDGER: u32 = 4_242;

/// Opt-in switch for rewriting fixtures. Never set in CI.
const UPDATE_ENV_VAR: &str = "SYNAPSE_UPDATE_EVENT_SNAPSHOTS";

const MINIMAL_WASM: &[u8] = include_bytes!("../testdata/minimal.wasm");

// ─── Snapshot comparison ─────────────────────────────────────────────────────

fn fixture_dir() -> StdString {
    format!("{}/fixtures/event_snapshots", env!("CARGO_MANIFEST_DIR"))
}

fn fixture_path(name: &str) -> StdString {
    format!("{}/{name}.snap", fixture_dir())
}

/// Build the full fixture text for a scenario from the events it emitted.
fn render_snapshot(name: &str, entry_point: &str, events: &[StdString]) -> StdString {
    let mut out = StdString::new();
    out.push_str(&format!("# event snapshot: {name}\n"));
    out.push_str(&format!("# entry point:    {entry_point}\n"));
    out.push_str(&format!(
        "# regenerate:     {UPDATE_ENV_VAR}=1 cargo test event_snapshot\n"
    ));
    if events.is_empty() {
        out.push_str("(no events)\n");
    }
    for line in events {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Compare `actual` against the committed fixture for `name`.
///
/// Returns `Err(diff)` on any mismatch, including a missing fixture. Only
/// when [`UPDATE_ENV_VAR`] is set does it (re)write the fixture instead.
fn check_snapshot(name: &str, actual: &str, allow_update: bool) -> Result<(), StdString> {
    let path = fixture_path(name);
    if allow_update && std::env::var(UPDATE_ENV_VAR).is_ok() {
        std::fs::create_dir_all(fixture_dir()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return Ok(());
    }
    let expected = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "missing event snapshot `{path}` ({e}).\n\
             Generate it deliberately with `{UPDATE_ENV_VAR}=1 cargo test event_snapshot` \
             and commit it after review.\n--- actual ---\n{actual}"
        )
    })?;
    match line_diff(&expected, actual) {
        None => Ok(()),
        Some(diff) => Err(format!(
            "event payload drift in `{name}` (fixture {path}).\n\
             If this change is intentional, it is an EVENTS.md-governed API change: \
             update EVENTS.md/CHANGELOG.md and regenerate with \
             `{UPDATE_ENV_VAR}=1 cargo test event_snapshot`.\n{diff}"
        )),
    }
}

fn assert_snapshot(name: &str, entry_point: &str, env: &Env) {
    let actual = render_snapshot(name, entry_point, &render_last_events(env));
    if let Err(msg) = check_snapshot(name, &actual, true) {
        panic!("{msg}");
    }
}

// ─── Deterministic scenario fixtures ─────────────────────────────────────────

struct Ctx {
    env: Env,
    client: SynapseCoreContractClient<'static>,
    admin: Address,
    relay: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.ledger().set_sequence_number(SNAPSHOT_LEDGER);
    let id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    Ctx {
        env,
        client,
        admin,
        relay,
    }
}

const G_ADDR: &str = "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ";

fn payload(env: &Env, tx_id: &str, idem: &str) -> CallbackPayload {
    let acct = String::from_str(env, G_ADDR);
    CallbackPayload {
        transaction_id: String::from_str(env, tx_id),
        stellar_account: acct.clone(),
        amount: 1_000,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: acct,
        idempotency_key: String::from_str(env, idem),
        anchor_transaction_id: String::from_str(env, "anchor-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    }
}

fn s(env: &Env, v: &str) -> String {
    String::from_str(env, v)
}

fn registered(c: &Ctx) -> String {
    c.client
        .register_callback(&payload(&c.env, "tx-1", "idem-1"))
}

fn processing(c: &Ctx) -> String {
    let id = registered(c);
    c.client.start_processing(&id, &c.relay);
    id
}

fn conflicting_evidence(env: &Env) -> SlashEvidence {
    let a = payload(env, "tx-1", "idem-a");
    let mut b = payload(env, "tx-1", "idem-b");
    b.amount = 2_000;
    SlashEvidence {
        tx_id: s(env, "tx-1"),
        payload_a: a,
        payload_b: b,
    }
}

// ─── One test per entry point / documented ordering variant ─────────────────

#[test]
fn event_snapshot_initialize() {
    let c = setup();
    assert_snapshot("initialize", "initialize", &c.env);
}

#[test]
fn event_snapshot_register_callback() {
    let c = setup();
    registered(&c);
    assert_snapshot("register_callback", "register_callback", &c.env);
}

/// Documented variant: an idempotent replay returns the original id and
/// emits nothing (EVENTS.md — `reg` fires on first write only).
#[test]
fn event_snapshot_register_callback_idempotent_replay() {
    let c = setup();
    registered(&c);
    registered(&c);
    assert_snapshot(
        "register_callback_idempotent_replay",
        "register_callback",
        &c.env,
    );
}

#[test]
fn event_snapshot_start_processing() {
    let c = setup();
    processing(&c);
    assert_snapshot("start_processing", "start_processing", &c.env);
}

/// Documented ordering: `status` then `done`.
#[test]
fn event_snapshot_complete_transaction() {
    let c = setup();
    let id = processing(&c);
    c.client
        .complete_transaction(&id, &s(&c.env, "abc123hash"), &c.relay);
    assert_snapshot("complete_transaction", "complete_transaction", &c.env);
}

/// Documented ordering: `status` then `fail`, from `Pending`.
#[test]
fn event_snapshot_fail_transaction_from_pending() {
    let c = setup();
    let id = registered(&c);
    c.client
        .fail_transaction(&id, &s(&c.env, "horizon_timeout"), &c.relay);
    assert_snapshot("fail_transaction_from_pending", "fail_transaction", &c.env);
}

/// Documented ordering: `status` then `fail`, from `Processing`.
#[test]
fn event_snapshot_fail_transaction_from_processing() {
    let c = setup();
    let id = processing(&c);
    c.client
        .fail_transaction(&id, &s(&c.env, "horizon_timeout"), &c.admin);
    assert_snapshot(
        "fail_transaction_from_processing",
        "fail_transaction",
        &c.env,
    );
}

#[test]
fn event_snapshot_propose_admin() {
    let c = setup();
    let nominee = Address::generate(&c.env);
    c.client.propose_admin(&nominee);
    assert_snapshot("propose_admin", "propose_admin", &c.env);
}

#[test]
fn event_snapshot_accept_admin() {
    let c = setup();
    let nominee = Address::generate(&c.env);
    c.client.propose_admin(&nominee);
    c.client.accept_admin(&nominee);
    assert_snapshot("accept_admin", "accept_admin", &c.env);
}

#[test]
fn event_snapshot_set_relay_signer() {
    let c = setup();
    let next = Address::generate(&c.env);
    c.client.set_relay_signer(&next);
    assert_snapshot("set_relay_signer", "set_relay_signer", &c.env);
}

#[test]
fn event_snapshot_upgrade() {
    let c = setup();
    let hash: BytesN<32> = c.env.deployer().upload_contract_wasm(MINIMAL_WASM);
    c.client.upgrade(&hash, &1);
    assert_snapshot("upgrade", "upgrade", &c.env);
}

#[test]
fn event_snapshot_pause() {
    let c = setup();
    c.client.pause();
    assert_snapshot("pause", "pause", &c.env);
}

#[test]
fn event_snapshot_unpause() {
    let c = setup();
    c.client.pause();
    c.client.unpause();
    assert_snapshot("unpause", "unpause", &c.env);
}

#[test]
fn event_snapshot_set_param() {
    let c = setup();
    c.client.set_param(&s(&c.env, "slash_bps"), &5_000);
    assert_snapshot("set_param", "set_param", &c.env);
}

#[test]
fn event_snapshot_bond_collateral_initial() {
    let c = setup();
    c.client.bond_collateral(&c.relay, &10_000);
    assert_snapshot("bond_collateral_initial", "bond_collateral", &c.env);
}

/// Variant: a top-up reports the increment in `amount` and the new total.
#[test]
fn event_snapshot_bond_collateral_topup() {
    let c = setup();
    c.client.bond_collateral(&c.relay, &10_000);
    c.client.bond_collateral(&c.relay, &2_500);
    assert_snapshot("bond_collateral_topup", "bond_collateral", &c.env);
}

#[test]
fn event_snapshot_unbond_collateral() {
    let c = setup();
    c.client.bond_collateral(&c.relay, &10_000);
    c.client.unbond_collateral(&c.relay, &4_000);
    assert_snapshot("unbond_collateral", "unbond_collateral", &c.env);
}

#[test]
fn event_snapshot_claim_unbond() {
    let c = setup();
    c.client.set_param(&s(&c.env, "unbond_delay_ledgers"), &10);
    c.client.bond_collateral(&c.relay, &10_000);
    c.client.unbond_collateral(&c.relay, &4_000);
    c.env.ledger().set_sequence_number(SNAPSHOT_LEDGER + 10);
    c.client.claim_unbond(&c.relay);
    assert_snapshot("claim_unbond", "claim_unbond", &c.env);
}

/// Variant: a partial slash leaves a non-zero `remaining_bond`.
#[test]
fn event_snapshot_slash_signer_partial() {
    let c = setup();
    c.client.set_param(&s(&c.env, "slash_bps"), &2_500);
    c.client.bond_collateral(&c.relay, &10_000);
    c.client
        .slash_signer(&c.relay, &conflicting_evidence(&c.env), &c.admin);
    assert_snapshot("slash_signer_partial", "slash_signer", &c.env);
}

/// Variant: the default 100 % slash reports `remaining_bond = 0`.
#[test]
fn event_snapshot_slash_signer_full() {
    let c = setup();
    c.client.bond_collateral(&c.relay, &10_000);
    c.client
        .slash_signer(&c.relay, &conflicting_evidence(&c.env), &c.admin);
    assert_snapshot("slash_signer_full", "slash_signer", &c.env);
}

#[test]
fn event_snapshot_set_anchor_tier() {
    let c = setup();
    let anchor = Address::generate(&c.env);
    c.client.set_anchor_tier(&anchor, &500, &s(&c.env, "gold"));
    assert_snapshot("set_anchor_tier", "set_anchor_tier", &c.env);
}

#[test]
fn event_snapshot_compute_effective_fee_with_tier() {
    let c = setup();
    let anchor = Address::generate(&c.env);
    c.client.set_anchor_tier(&anchor, &500, &s(&c.env, "gold"));
    c.client.compute_effective_fee(&anchor, &10_000);
    assert_snapshot(
        "compute_effective_fee_with_tier",
        "compute_effective_fee",
        &c.env,
    );
}

/// Variant: no tier → `rebate_bps = 0`, `effective_fee == base_fee`.
#[test]
fn event_snapshot_compute_effective_fee_no_tier() {
    let c = setup();
    let anchor = Address::generate(&c.env);
    c.client.compute_effective_fee(&anchor, &10_000);
    assert_snapshot(
        "compute_effective_fee_no_tier",
        "compute_effective_fee",
        &c.env,
    );
}

/// `EventDisputeRaised` is schema-locked in EVENTS.md but has no entry point
/// yet; pin the emitter's payload directly so wiring it later cannot change
/// the shape silently.
#[test]
fn event_snapshot_dispute_raised_emitter() {
    let c = setup();
    let caller = c.admin.clone();
    c.env.as_contract(&c.client.address, || {
        EventEmitter::dispute_raised(
            &c.env,
            &s(&c.env, "tx-1"),
            &s(&c.env, "chargeback"),
            &caller,
        );
    });
    assert_snapshot(
        "dispute_raised_emitter",
        "(emitter only; dispute_transaction not wired)",
        &c.env,
    );
}

/// See [`event_snapshot_dispute_raised_emitter`].
#[test]
fn event_snapshot_dispute_resolved_emitter() {
    let c = setup();
    let caller = c.admin.clone();
    c.env.as_contract(&c.client.address, || {
        EventEmitter::dispute_resolved(&c.env, &s(&c.env, "tx-1"), true, &caller);
    });
    assert_snapshot(
        "dispute_resolved_emitter",
        "(emitter only; resolve_dispute not wired)",
        &c.env,
    );
}

// ─── Proof that the gate actually fails ──────────────────────────────────────

/// A changed payload *value* against the committed `register_callback`
/// fixture must be reported as drift, with a diff naming the changed lines.
#[test]
fn event_snapshot_gate_detects_value_drift() {
    let c = setup();
    let mut p = payload(&c.env, "tx-1", "idem-1");
    p.amount = 1_001; // fixture pins 1_000
    c.client.register_callback(&p);
    let actual = render_snapshot(
        "register_callback",
        "register_callback",
        &render_last_events(&c.env),
    );
    let err = check_snapshot("register_callback", &actual, false)
        .expect_err("a changed payload value must fail the snapshot gate");
    assert!(err.contains("- data:") && err.contains("+ data:"), "{err}");
    assert!(err.contains("i128:1001"), "{err}");
}

/// A changed payload *shape* (an extra field appended to the struct, the
/// classic "additive" change that still needs an EVENTS.md review) must be
/// reported as drift.
#[test]
fn event_snapshot_gate_detects_shape_drift() {
    use soroban_sdk::{contracttype, symbol_short};

    #[contracttype]
    struct DriftedStatusChanged {
        tx_id: String,
        old_status: crate::types::TransactionStatus,
        new_status: crate::types::TransactionStatus,
        ledger: u32,
        reason: Option<String>,
    }

    let c = setup();
    let id = registered(&c);
    c.env.as_contract(&c.client.address, || {
        c.env.events().publish(
            (symbol_short!("synapse"), symbol_short!("status")),
            DriftedStatusChanged {
                tx_id: id.clone(),
                old_status: crate::types::TransactionStatus::Pending,
                new_status: crate::types::TransactionStatus::Processing,
                ledger: SNAPSHOT_LEDGER,
                reason: None,
            },
        );
    });
    let actual = render_snapshot(
        "start_processing",
        "start_processing",
        &render_last_events(&c.env),
    );
    let err = check_snapshot("start_processing", &actual, false)
        .expect_err("an added payload field must fail the snapshot gate");
    assert!(err.contains("reason: void"), "{err}");
}

/// A scenario with no committed fixture must fail, not silently pass or
/// auto-create one.
#[test]
fn event_snapshot_gate_rejects_missing_fixture() {
    let err = check_snapshot("__no_such_fixture__", "anything\n", false)
        .expect_err("a missing fixture must fail the snapshot gate");
    assert!(err.contains("missing event snapshot"), "{err}");
}

// ─── Catalogue coverage (EVENTS.md §2) ───────────────────────────────────────

/// Topics catalogued in EVENTS.md §2 that have **no** snapshot because the
/// contract has no code emitting them. Each is a tracked finding, not an
/// oversight: the entry points were lost in the #176–#197 squash merges and
/// their restoration is tracked in #199 (the new snapshots land with them).
const KNOWN_GAPS: &[(&str, &str)] = &[
    ("batch", "batch_register_callback lost in #176 merge (#199)"),
    ("up_prop", "timelocked upgrade lost in #190 merge (#199)"),
    ("uprop", "timelocked upgrade lost in #190 merge (#199)"),
    ("up_fin", "timelocked upgrade lost in #190 merge (#199)"),
    ("up_can", "timelocked upgrade lost in #190 merge (#199)"),
    ("rollback", "rollback_upgrade lost in #190 merge (#199)"),
    ("migrate", "upgrade_and_migrate lost in #190 merge (#199)"),
    (
        "chk_pass",
        "post-upgrade self-check lost in #191 merge (#199)",
    ),
    (
        "chk_fail",
        "post-upgrade self-check lost in #191 merge (#199)",
    ),
    ("expire", "expire_transaction has no entry point (#199)"),
    ("uqset", "upgrade quorum lost in #187 merge (#199)"),
    ("attest", "set_signer_attestation has no entry point (#199)"),
    ("renounce", "renounce_admin has no entry point (#199)"),
    ("guardian", "set_guardian has no entry point (#199)"),
    ("apause", "trip_auto_pause has no entry point (#199)"),
    ("aunpause", "unpause_auto has no entry point (#199)"),
];

/// Topics the contract emits (and that are snapshotted above) but that
/// EVENTS.md §2 does not yet catalogue — the Wave 2 events from #143–#146
/// were merged without catalogue rows. Tracked finding: EVENTS.md needs rows
/// for these before subscribers can rely on them.
const UNCATALOGUED_TOPICS: &[&str] = &[
    "param", "bonded", "unbondrq", "unbondcl", "slashed", "tierset", "rebate",
];

/// Extract the `Topic[1]` column of the EVENTS.md §2 status table.
fn catalogued_topics(events_md: &str) -> BTreeSet<StdString> {
    let mut in_table = false;
    let mut topics = BTreeSet::new();
    for line in events_md.lines() {
        if line.starts_with("## 2.") {
            in_table = true;
            continue;
        }
        if in_table && line.starts_with("## ") {
            break;
        }
        if !in_table || !line.starts_with("| [`Event") {
            continue;
        }
        let cols: Vec<&str> = line.split('|').map(str::trim).collect();
        // cols[0] is empty (leading pipe); cols[2] is the topic column.
        topics.insert(cols[2].trim_matches('`').to_string());
    }
    topics
}

/// Topics covered by the committed fixtures (`topics: [sym:synapse, sym:X]`).
fn snapshotted_topics() -> BTreeSet<StdString> {
    let mut topics = BTreeSet::new();
    for entry in std::fs::read_dir(fixture_dir()).expect("fixture dir exists") {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("topics: [sym:synapse, sym:") {
                topics.insert(rest.trim_end_matches(']').to_string());
            }
        }
    }
    topics
}

#[test]
fn every_catalogued_event_has_a_snapshot_or_a_tracked_gap() {
    let catalogued = catalogued_topics(include_str!("../EVENTS.md"));
    assert!(
        catalogued.len() >= 10,
        "EVENTS.md §2 parser found only {} topics — did the table format change?",
        catalogued.len()
    );
    let covered = snapshotted_topics();
    let gaps: BTreeSet<StdString> = KNOWN_GAPS.iter().map(|(t, _)| t.to_string()).collect();
    let uncatalogued: BTreeSet<StdString> =
        UNCATALOGUED_TOPICS.iter().map(|t| t.to_string()).collect();

    let untracked: Vec<_> = catalogued
        .iter()
        .filter(|t| !covered.contains(*t) && !gaps.contains(*t))
        .collect();
    assert!(
        untracked.is_empty(),
        "EVENTS.md catalogues topics with neither a snapshot nor a KNOWN_GAPS entry: {untracked:?}"
    );

    let stale_gaps: Vec<_> = gaps.iter().filter(|t| covered.contains(*t)).collect();
    assert!(
        stale_gaps.is_empty(),
        "these KNOWN_GAPS now have snapshots — remove them from the list: {stale_gaps:?}"
    );

    let dropped_gaps: Vec<_> = gaps.iter().filter(|t| !catalogued.contains(*t)).collect();
    assert!(
        dropped_gaps.is_empty(),
        "these KNOWN_GAPS are no longer catalogued in EVENTS.md — remove them: {dropped_gaps:?}"
    );

    let actual_uncatalogued: BTreeSet<StdString> =
        covered.difference(&catalogued).cloned().collect();
    assert_eq!(
        actual_uncatalogued, uncatalogued,
        "snapshotted-but-uncatalogued topics changed; update EVENTS.md §2 or UNCATALOGUED_TOPICS"
    );
}
