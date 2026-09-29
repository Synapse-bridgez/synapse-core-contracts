//! # Differential testing: `release` vs `release-with-logs` (#131)
//!
//! The two release profiles in `Cargo.toml` differ only in
//! `debug-assertions`. If contract behaviour ever depends on that flag (a
//! `debug_assert!` with a side effect, or a `cfg!(debug_assertions)` branch
//! that touches state), the profile operators test with and the profile they
//! ship would silently diverge. Passing each profile's own tests does not
//! catch that; comparing their observable behaviour does.
//!
//! ## How it works
//!
//! [`behaviour_trace`] runs a fixed corpus that calls every `#[contractimpl]`
//! entry point, including error and overflow paths, and records for each
//! call:
//!
//! * the full return value (`Ok` payload or error code),
//! * every event it emitted (readable + raw XDR), and
//! * a digest of the complete ledger state afterwards,
//!
//! then dumps every ledger entry byte-for-byte at the end.
//! `scripts/diff_profiles.sh` builds the test binary under each profile, has
//! [`profile_diff_emit_trace`] write the trace to a file, and `diff`s the two
//! files. CI runs it on every PR (`profile-diff` job in
//! `.github/workflows/rust.yml`).
//!
//! ## Proof that it catches a divergence
//!
//! * `scripts/diff_profiles.sh --self-test` sets
//!   `SYNAPSE_PROFILE_DIFF_FIXTURE=1`, which adds a call to
//!   [`DivergenceFixture::probe`] to the corpus. That contract writes to
//!   storage only when `debug_assertions` is on, and never mentions it in its
//!   return value or events. The self-test passes only if the diff **fails**.
//!   CI runs it next to the real comparison.
//! * [`trace_detects_state_only_divergence`] proves in-process that a change
//!   visible *only* in storage (no event, same return value) changes the
//!   trace.
//! * [`trace_is_deterministic`] rules out false positives from
//!   nondeterminism.

#![cfg(test)]

extern crate std;

use std::{format, string::String as StdString, vec::Vec};

use soroban_sdk::{
    contract, contractimpl, symbol_short,
    testutils::{Address as _, EnvTestConfig, Ledger},
    Address, BytesN, Env, String,
};

use crate::test_support::{fnv1a64, render_last_events, render_ledger_state};
use crate::types::{CallbackPayload, CallbackType, SlashEvidence};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

/// Env var naming the file [`profile_diff_emit_trace`] writes the trace to.
const TRACE_OUT_ENV: &str = "SYNAPSE_PROFILE_TRACE_OUT";
/// Env var that adds the deliberately divergent fixture to the corpus.
const FIXTURE_ENV: &str = "SYNAPSE_PROFILE_DIFF_FIXTURE";

const G_ADDR: &str = "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ";

// ─── Divergence fixture ──────────────────────────────────────────────────────

/// A deliberately profile-dependent contract, used only to prove the harness
/// catches this class of bug. It returns the same value and emits nothing in
/// both profiles; the difference is visible only in ledger state.
#[contract]
pub struct DivergenceFixture;

#[contractimpl]
impl DivergenceFixture {
    /// Returns `7`. Under `debug_assertions` it also writes a storage entry.
    /// That is the bug class under test: debug-only code with a side effect.
    pub fn probe(env: Env) -> u32 {
        if cfg!(debug_assertions) {
            env.storage().instance().set(&symbol_short!("dbg"), &true);
        }
        7
    }
}

// ─── Trace recorder ──────────────────────────────────────────────────────────

struct Trace {
    lines: Vec<StdString>,
    step: u32,
}

impl Trace {
    fn new() -> Self {
        Trace {
            lines: Vec::new(),
            step: 0,
        }
    }

    /// Record one invocation: its result, the events it emitted (successful
    /// invocations only; a failed one rolls its events back), and a digest of
    /// the full ledger state afterwards.
    fn record(&mut self, env: &Env, name: &str, result: StdString, succeeded: bool) {
        self.step += 1;
        self.lines
            .push(format!("#{:03} {name} => {result}", self.step));
        if succeeded {
            for line in render_last_events(env) {
                self.lines.push(format!("    {line}"));
            }
        }
        let state = render_ledger_state(env).join("\n");
        self.lines
            .push(format!("    state: {:016x}", fnv1a64(state.as_bytes())));
    }

    fn finish(mut self, env: &Env) -> StdString {
        self.lines.push(format!("steps: {}", self.step));
        self.lines.push("final ledger state:".into());
        for row in render_ledger_state(env) {
            self.lines.push(format!("    {row}"));
        }
        let mut out = self.lines.join("\n");
        out.push('\n');
        out
    }
}

/// Invoke a `try_*` client call and record it.
macro_rules! rec {
    ($t:expr, $env:expr, $name:expr, $call:expr) => {{
        let r = $call;
        let ok = matches!(r, Ok(Ok(_)));
        $t.record($env, $name, format!("{:?}", r), ok);
    }};
}

// ─── Corpus ──────────────────────────────────────────────────────────────────

fn s(env: &Env, v: &str) -> String {
    String::from_str(env, v)
}

fn payload(env: &Env, tx: &str, idem: &str) -> CallbackPayload {
    CallbackPayload {
        transaction_id: s(env, tx),
        stellar_account: s(env, G_ADDR),
        amount: 1_000,
        asset_code: s(env, "USDC"),
        asset_issuer: s(env, G_ADDR),
        idempotency_key: s(env, idem),
        anchor_transaction_id: s(env, "anchor-1"),
        callback_type: CallbackType::Deposit,
        callback_status: s(env, "pending_external"),
    }
}

/// A named mutation applied to a valid payload to trigger one validation error.
type PayloadEdit = fn(&Env, &mut CallbackPayload);

fn fresh_env() -> Env {
    let env = Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    env.ledger().set_sequence_number(1_000);
    env.mock_all_auths();
    env
}

/// Run the corpus and return its trace.
///
/// `extra` runs after the corpus and before the final state dump. The
/// in-process self-check uses it to inject a divergence.
fn behaviour_trace_with(include_fixture: bool, extra: impl FnOnce(&Env, &Address)) -> StdString {
    let env = fresh_env();
    let id = env.register(SynapseCoreContract, ());
    let c = SynapseCoreContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    let stranger = Address::generate(&env);
    let e = &env;
    let mut t = Trace::new();

    // Initialisation.
    rec!(t, e, "health (pre-init)", c.try_health());
    rec!(t, e, "admin (pre-init)", c.try_admin());
    rec!(t, e, "initialize", c.try_initialize(&admin, &relay));
    rec!(t, e, "initialize (again)", c.try_initialize(&admin, &relay));
    rec!(t, e, "health", c.try_health());
    rec!(t, e, "version", c.try_version());
    rec!(t, e, "schema_version", c.try_schema_version());
    rec!(t, e, "admin", c.try_admin());
    rec!(t, e, "relay_signer", c.try_relay_signer());

    // Ingestion: happy path, replay, duplicate id, every validation error.
    let p1 = payload(e, "tx-1", "idem-1");
    rec!(t, e, "register tx-1", c.try_register_callback(&p1));
    rec!(t, e, "register tx-1 (replay)", c.try_register_callback(&p1));
    rec!(
        t,
        e,
        "is_duplicate idem-1",
        c.try_is_duplicate(&s(e, "idem-1"))
    );
    rec!(
        t,
        e,
        "register tx-1 (new idem, same id)",
        c.try_register_callback(&payload(e, "tx-1", "idem-1b"))
    );
    let invalid: [(&str, PayloadEdit); 7] = [
        ("bad account", |e, p| p.stellar_account = s(e, "GBAD")),
        ("zero amount", |_, p| p.amount = 0),
        ("bad asset", |e, p| p.asset_code = s(e, "usdc")),
        ("bad issuer", |e, p| p.asset_issuer = s(e, "GBAD")),
        ("empty idem", |e, p| p.idempotency_key = s(e, "")),
        ("long tx id", |e, p| {
            p.transaction_id = s(e, &"x".repeat(65))
        }),
        ("long status", |e, p| {
            p.callback_status = s(e, &"x".repeat(33))
        }),
    ];
    for (label, edit) in invalid {
        let mut p = payload(e, "tx-bad", "idem-bad");
        edit(e, &mut p);
        rec!(
            t,
            e,
            &format!("register ({label})"),
            c.try_register_callback(&p)
        );
    }
    rec!(
        t,
        e,
        "register tx-2",
        c.try_register_callback(&payload(e, "tx-2", "idem-2"))
    );
    rec!(
        t,
        e,
        "register tx-3",
        c.try_register_callback(&payload(e, "tx-3", "idem-3"))
    );

    // Lifecycle, including illegal transitions and unauthorised callers.
    let tx1 = s(e, "tx-1");
    let tx2 = s(e, "tx-2");
    let tx3 = s(e, "tx-3");
    let hash = s(e, "abc123");
    let reason = s(e, "horizon_timeout");
    e.ledger().set_sequence_number(1_010);
    rec!(
        t,
        e,
        "start tx-1 (stranger)",
        c.try_start_processing(&tx1, &stranger)
    );
    rec!(t, e, "start tx-1", c.try_start_processing(&tx1, &relay));
    rec!(
        t,
        e,
        "start tx-1 (again)",
        c.try_start_processing(&tx1, &relay)
    );
    rec!(
        t,
        e,
        "complete tx-2 (pending)",
        c.try_complete_transaction(&tx2, &hash, &relay)
    );
    rec!(
        t,
        e,
        "complete tx-1 (hash too long)",
        c.try_complete_transaction(&tx1, &s(e, &"h".repeat(73)), &relay)
    );
    rec!(
        t,
        e,
        "complete tx-1",
        c.try_complete_transaction(&tx1, &hash, &admin)
    );
    rec!(
        t,
        e,
        "fail tx-1 (completed)",
        c.try_fail_transaction(&tx1, &reason, &relay)
    );
    rec!(
        t,
        e,
        "fail tx-2 (pending)",
        c.try_fail_transaction(&tx2, &reason, &relay)
    );
    rec!(t, e, "start tx-3", c.try_start_processing(&tx3, &admin));
    rec!(
        t,
        e,
        "fail tx-3 (processing)",
        c.try_fail_transaction(&tx3, &reason, &relay)
    );
    rec!(
        t,
        e,
        "start missing",
        c.try_start_processing(&s(e, "nope"), &relay)
    );
    rec!(t, e, "get_transaction tx-1", c.try_get_transaction(&tx1));
    rec!(t, e, "get_status tx-3", c.try_get_status(&tx3));

    // Pause.
    rec!(t, e, "pause", c.try_pause());
    rec!(t, e, "is_paused", c.try_is_paused());
    rec!(
        t,
        e,
        "register (paused)",
        c.try_register_callback(&payload(e, "tx-4", "idem-4"))
    );
    rec!(t, e, "unpause", c.try_unpause());

    // Admin transfer, relay rotation, upgrade guard.
    let new_admin = Address::generate(e);
    rec!(
        t,
        e,
        "accept_admin (none pending)",
        c.try_accept_admin(&new_admin)
    );
    rec!(t, e, "propose_admin (self)", c.try_propose_admin(&id));
    rec!(t, e, "propose_admin", c.try_propose_admin(&new_admin));
    rec!(t, e, "pending_admin", c.try_pending_admin());
    rec!(
        t,
        e,
        "accept_admin (wrong caller)",
        c.try_accept_admin(&stranger)
    );
    rec!(t, e, "accept_admin", c.try_accept_admin(&new_admin));
    let new_relay = Address::generate(e);
    rec!(t, e, "set_relay_signer", c.try_set_relay_signer(&new_relay));
    rec!(
        t,
        e,
        "start tx-2 (stale relay)",
        c.try_start_processing(&tx2, &relay)
    );
    rec!(
        t,
        e,
        "upgrade (schema mismatch)",
        c.try_upgrade(&BytesN::from_array(e, &[7u8; 32]), &99)
    );

    // Param registry.
    rec!(
        t,
        e,
        "get_param (unset)",
        c.try_get_param(&s(e, "slash_bps"))
    );
    rec!(t, e, "set_param (empty)", c.try_set_param(&s(e, ""), &1));
    rec!(
        t,
        e,
        "set_param slash_bps",
        c.try_set_param(&s(e, "slash_bps"), &2_500)
    );
    rec!(
        t,
        e,
        "set_param unbond_delay",
        c.try_set_param(&s(e, "unbond_delay_ledgers"), &5)
    );
    rec!(
        t,
        e,
        "get_param slash_bps",
        c.try_get_param(&s(e, "slash_bps"))
    );

    // Collateral, including i128 overflow (overflow-checks are on in both
    // profiles; a divergence here would mean that stopped being true).
    rec!(t, e, "bond (zero)", c.try_bond_collateral(&new_relay, &0));
    rec!(t, e, "bond", c.try_bond_collateral(&new_relay, &10_000));
    rec!(
        t,
        e,
        "bond (top-up)",
        c.try_bond_collateral(&new_relay, &5_000)
    );
    rec!(
        t,
        e,
        "bond (overflow)",
        c.try_bond_collateral(&new_relay, &i128::MAX)
    );
    rec!(
        t,
        e,
        "unbond (too much)",
        c.try_unbond_collateral(&new_relay, &20_000)
    );
    rec!(t, e, "unbond", c.try_unbond_collateral(&new_relay, &3_000));
    rec!(
        t,
        e,
        "unbond (pending)",
        c.try_unbond_collateral(&new_relay, &1)
    );
    rec!(t, e, "claim (early)", c.try_claim_unbond(&new_relay));
    e.ledger().set_sequence_number(1_015);
    rec!(t, e, "claim", c.try_claim_unbond(&new_relay));
    rec!(t, e, "get_bond_record", c.try_get_bond_record(&new_relay));
    rec!(
        t,
        e,
        "get_unbond_request",
        c.try_get_unbond_request(&new_relay)
    );

    // Slashing.
    let a = payload(e, "tx-9", "idem-a");
    let mut b = payload(e, "tx-9", "idem-b");
    let same = SlashEvidence {
        tx_id: s(e, "tx-9"),
        payload_a: a.clone(),
        payload_b: a.clone(),
    };
    rec!(
        t,
        e,
        "slash (not conflicting)",
        c.try_slash_signer(&new_relay, &same, &new_admin)
    );
    b.amount = 2_000;
    let evidence = SlashEvidence {
        tx_id: s(e, "tx-9"),
        payload_a: a,
        payload_b: b,
    };
    rec!(
        t,
        e,
        "slash (unbonded signer)",
        c.try_slash_signer(&stranger, &evidence, &new_admin)
    );
    rec!(
        t,
        e,
        "slash",
        c.try_slash_signer(&new_relay, &evidence, &new_admin)
    );
    rec!(
        t,
        e,
        "bond (huge)",
        c.try_bond_collateral(&stranger, &(i128::MAX / 2))
    );
    rec!(
        t,
        e,
        "slash (mul overflow)",
        c.try_slash_signer(&stranger, &evidence, &new_admin)
    );

    // Anchor tiers and fees.
    let anchor = Address::generate(e);
    rec!(
        t,
        e,
        "get_anchor_tier (unset)",
        c.try_get_anchor_tier(&anchor)
    );
    rec!(
        t,
        e,
        "set_anchor_tier (bps)",
        c.try_set_anchor_tier(&anchor, &10_001, &s(e, "gold"))
    );
    rec!(
        t,
        e,
        "set_anchor_tier (label)",
        c.try_set_anchor_tier(&anchor, &500, &s(e, &"l".repeat(17)))
    );
    rec!(
        t,
        e,
        "set_anchor_tier",
        c.try_set_anchor_tier(&anchor, &500, &s(e, "gold"))
    );
    rec!(
        t,
        e,
        "fee (tier)",
        c.try_compute_effective_fee(&anchor, &10_000)
    );
    rec!(
        t,
        e,
        "fee (no tier)",
        c.try_compute_effective_fee(&stranger, &10_000)
    );
    rec!(
        t,
        e,
        "fee (negative)",
        c.try_compute_effective_fee(&anchor, &-1)
    );
    rec!(
        t,
        e,
        "fee (mul overflow)",
        c.try_compute_effective_fee(&anchor, &i128::MAX)
    );

    if include_fixture {
        let fid = e.register(DivergenceFixture, ());
        let f = DivergenceFixtureClient::new(e, &fid);
        rec!(t, e, "fixture probe", f.try_probe());
    }

    extra(e, &id);
    t.finish(e)
}

fn behaviour_trace(include_fixture: bool) -> StdString {
    behaviour_trace_with(include_fixture, |_, _| {})
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// Entry point for `scripts/diff_profiles.sh`: writes the trace for the
/// current build profile to `$SYNAPSE_PROFILE_TRACE_OUT`. Under a plain
/// `cargo test` (variable unset) it still runs the whole corpus, so the
/// harness can't rot unnoticed.
#[test]
fn profile_diff_emit_trace() {
    let include_fixture = std::env::var(FIXTURE_ENV).is_ok_and(|v| v == "1");
    let trace = behaviour_trace(include_fixture);
    assert!(
        trace.contains("steps: ") && trace.lines().count() > 100,
        "trace unexpectedly small; corpus did not run"
    );
    if let Ok(path) = std::env::var(TRACE_OUT_ENV) {
        std::fs::write(&path, &trace).expect("write trace file");
    }
}

/// Two runs in the same build produce byte-identical traces. Without this,
/// a cross-profile diff could fail for reasons unrelated to the profile.
#[test]
fn trace_is_deterministic() {
    assert_eq!(behaviour_trace(false), behaviour_trace(false));
}

/// A divergence visible **only** in ledger state (no event, no return value)
/// must change the trace. This proves the state dump is part of the
/// comparison and not just decoration.
#[test]
fn trace_detects_state_only_divergence() {
    let baseline = behaviour_trace(false);
    let diverged = behaviour_trace_with(false, |env, id| {
        env.as_contract(id, || {
            env.storage().instance().set(&symbol_short!("dbg"), &true);
        });
    });
    let diff = crate::test_support::line_diff(&baseline, &diverged)
        .expect("a state-only change must alter the trace");
    assert!(diff.lines().any(|l| l.starts_with('+')), "{diff}");
}

/// The fixture really is profile-dependent: its storage write happens iff
/// this build has `debug_assertions`, while the return value doesn't change.
/// That is exactly what the cross-profile self-test relies on.
#[test]
fn fixture_side_effect_tracks_debug_assertions() {
    let env = fresh_env();
    let fid = env.register(DivergenceFixture, ());
    assert_eq!(DivergenceFixtureClient::new(&env, &fid).probe(), 7);
    let wrote = env.as_contract(&fid, || env.storage().instance().has(&symbol_short!("dbg")));
    assert_eq!(wrote, cfg!(debug_assertions));
}
