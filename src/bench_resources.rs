//! # Resource-usage regression gate (#119)
//!
//! Meters every hot `#[contractimpl]` entry point against the **release WASM**
//! and compares CPU-instruction / memory-byte cost with the committed
//! [`resource_baseline.toml`](../resource_baseline.toml). The build fails when
//! any entry point exceeds its baseline by more than the configured threshold.
//!
//! ## Why the release WASM, not the native contract
//!
//! Native `cargo test` runs contract code as plain Rust, so the Soroban budget
//! only sees host calls; guest-side work (strkey base32 + CRC16, loops,
//! allocations in linear memory) costs nothing there. Registering the built
//! `.wasm` makes the host meter every executed WASM instruction and linear-memory
//! grow, which is what validators charge for on-chain.
//!
//! ## Determinism
//!
//! Soroban metering is a deterministic cost model (instruction counts, not wall
//! time), so the same WASM + SDK version yields identical numbers on every
//! machine; CI runner noise does not apply. The numbers *do* move when the
//! WASM changes — including a different `rustc` producing different code —
//! which is why the CI job pins the toolchain (see `.github/workflows/`).
//!
//! ## Running
//!
//! The gate is a no-op in plain `cargo test`; it is driven by
//! `scripts/check_resource_budget.sh` (or `make resource-gate`), which sets:
//!
//! | Variable                          | Meaning                                                  |
//! |-----------------------------------|----------------------------------------------------------|
//! | `SYNAPSE_RESOURCE_GATE=1`         | enable the gate                                          |
//! | `SYNAPSE_RESOURCE_WASM`           | path to the release WASM                                 |
//! | `SYNAPSE_RESOURCE_BASELINE`       | path to the baseline file                                |
//! | `SYNAPSE_RESOURCE_THRESHOLD_PCT`  | override the baseline's `threshold_pct`                  |
//! | `SYNAPSE_RESOURCE_UPDATE=1`       | rewrite the baseline from the current measurements       |
//! | `SYNAPSE_RESOURCE_INJECT`         | `scenario:pct` — inflate one measurement (CI self-test)  |

#![cfg(test)]

extern crate std;

use std::{
    borrow::ToOwned,
    collections::BTreeMap,
    eprintln, format,
    string::{String as StdString, ToString},
    vec::Vec,
};

use soroban_sdk::{testutils::Address as _, Address, Env, String};

use crate::types::{CallbackPayload, CallbackType};
use crate::SynapseCoreContractClient;

/// Default threshold used when the baseline file omits `threshold_pct`.
const DEFAULT_THRESHOLD_PCT: u64 = 15;

/// Checksum-valid SEP-23 G-address.
const G_VALID: &str = "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ";
/// Same shape as [`G_VALID`] with one character changed so only CRC16 fails —
/// the most expensive way for a strkey to be rejected.
const G_BAD_CRC: &str = "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGY";

// ─── Measurement ─────────────────────────────────────────────────────────────

/// CPU-instruction and memory-byte cost of one metered call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cost {
    pub cpu_insns: u64,
    pub mem_bytes: u64,
}

/// Fresh env with the release WASM registered and initialised.
struct Harness {
    env: Env,
    client: SynapseCoreContractClient<'static>,
    relay: Address,
}

impl Harness {
    fn new(wasm: &[u8]) -> Self {
        let env = Env::default();
        env.cost_estimate().budget().reset_unlimited();
        env.mock_all_auths();
        let contract_id = env.register(wasm, ());
        let client = SynapseCoreContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let relay = Address::generate(&env);
        client.initialize(&admin, &relay);
        Harness { env, client, relay }
    }

    /// Meter exactly the calls made inside `f`.
    fn measure(&self, f: impl FnOnce()) -> Cost {
        let mut budget = self.env.cost_estimate().budget();
        budget.reset_unlimited();
        f();
        Cost {
            cpu_insns: budget.cpu_instruction_cost(),
            mem_bytes: budget.memory_bytes_cost(),
        }
    }

    fn s(&self, v: &str) -> String {
        String::from_str(&self.env, v)
    }

    fn payload(&self, tx_id: &str, idem: &str) -> CallbackPayload {
        CallbackPayload {
            transaction_id: self.s(tx_id),
            stellar_account: self.s(G_VALID),
            amount: 10_000_000,
            asset_code: self.s("USDC"),
            asset_issuer: self.s(G_VALID),
            idempotency_key: self.s(idem),
            anchor_transaction_id: self.s("anchor-tx-abc123"),
            callback_type: CallbackType::Deposit,
            callback_status: self.s("pending_external"),
        }
    }

    /// Register `tx-1` and return its id.
    fn registered(&self) -> String {
        self.client
            .register_callback(&self.payload("tx-1", "idem-1"))
    }
}

/// A named, self-contained scenario: builds its own preconditions, then meters
/// only the entry-point call under test.
struct Scenario {
    name: &'static str,
    run: fn(&Harness) -> Cost,
}

/// Every metered scenario. Names are the keys in `resource_baseline.toml`.
///
/// `register_callback_reject_*` cover each validation failure path (#120):
/// they meter how cheaply malformed input is turned away.
fn scenarios() -> Vec<Scenario> {
    Vec::from([
        Scenario {
            name: "register_callback",
            run: |h| {
                let p = h.payload("tx-1", "idem-1");
                h.measure(|| {
                    h.client.register_callback(&p);
                })
            },
        },
        Scenario {
            name: "register_callback_idempotent_replay",
            run: |h| {
                let p = h.payload("tx-1", "idem-1");
                h.client.register_callback(&p);
                h.measure(|| {
                    h.client.register_callback(&p);
                })
            },
        },
        Scenario {
            name: "register_callback_reject_amount",
            run: |h| {
                let mut p = h.payload("tx-1", "idem-1");
                p.amount = 0;
                h.measure(|| {
                    assert!(h.client.try_register_callback(&p).is_err());
                })
            },
        },
        Scenario {
            name: "register_callback_reject_idempotency_key",
            run: |h| {
                let p = h.payload("tx-1", "");
                h.measure(|| {
                    assert!(h.client.try_register_callback(&p).is_err());
                })
            },
        },
        Scenario {
            name: "register_callback_reject_callback_status_len",
            run: |h| {
                let mut p = h.payload("tx-1", "idem-1");
                p.callback_status = h.s("x".repeat(33).as_str());
                h.measure(|| {
                    assert!(h.client.try_register_callback(&p).is_err());
                })
            },
        },
        Scenario {
            name: "register_callback_reject_asset_code",
            run: |h| {
                let mut p = h.payload("tx-1", "idem-1");
                p.asset_code = h.s("usdc");
                h.measure(|| {
                    assert!(h.client.try_register_callback(&p).is_err());
                })
            },
        },
        Scenario {
            name: "register_callback_reject_account_checksum",
            run: |h| {
                let mut p = h.payload("tx-1", "idem-1");
                p.stellar_account = h.s(G_BAD_CRC);
                h.measure(|| {
                    assert!(h.client.try_register_callback(&p).is_err());
                })
            },
        },
        Scenario {
            name: "register_callback_reject_issuer_checksum",
            run: |h| {
                let mut p = h.payload("tx-1", "idem-1");
                p.asset_issuer = h.s(G_BAD_CRC);
                h.measure(|| {
                    assert!(h.client.try_register_callback(&p).is_err());
                })
            },
        },
        Scenario {
            name: "start_processing",
            run: |h| {
                let id = h.registered();
                h.measure(|| h.client.start_processing(&id, &h.relay))
            },
        },
        Scenario {
            name: "complete_transaction",
            run: |h| {
                let id = h.registered();
                h.client.start_processing(&id, &h.relay);
                let hash = h.s("d3b07384d113edec49eaa6238ad5ff00a975b0a5b2c6b2b13d7b6e6e10f7b3c1");
                h.measure(|| h.client.complete_transaction(&id, &hash, &h.relay))
            },
        },
        Scenario {
            name: "fail_transaction",
            run: |h| {
                let id = h.registered();
                let reason = h.s("horizon_timeout");
                h.measure(|| h.client.fail_transaction(&id, &reason, &h.relay))
            },
        },
        Scenario {
            name: "get_transaction",
            run: |h| {
                let id = h.registered();
                h.measure(|| {
                    h.client.get_transaction(&id);
                })
            },
        },
        Scenario {
            name: "propose_admin",
            run: |h| {
                let nominee = Address::generate(&h.env);
                h.measure(|| h.client.propose_admin(&nominee))
            },
        },
        Scenario {
            name: "accept_admin",
            run: |h| {
                let nominee = Address::generate(&h.env);
                h.client.propose_admin(&nominee);
                h.measure(|| h.client.accept_admin(&nominee))
            },
        },
        Scenario {
            name: "set_relay_signer",
            run: |h| {
                let signer = Address::generate(&h.env);
                h.measure(|| h.client.set_relay_signer(&signer))
            },
        },
        Scenario {
            name: "pause",
            run: |h| h.measure(|| h.client.pause()),
        },
        Scenario {
            name: "unpause",
            run: |h| {
                h.client.pause();
                h.measure(|| h.client.unpause())
            },
        },
        Scenario {
            name: "set_param",
            run: |h| {
                let name = h.s("slash_bps");
                h.measure(|| h.client.set_param(&name, &5_000))
            },
        },
        Scenario {
            name: "bond_collateral",
            run: |h| h.measure(|| h.client.bond_collateral(&h.relay, &1_000)),
        },
    ])
}

/// Name of the scenario recording the fixed per-invocation cost.
const OVERHEAD: &str = "invocation_overhead";

/// Run every scenario in its own fresh env so no scenario's storage or VM
/// cache state leaks into another's numbers.
///
/// Each call pays a fixed ~3M-instruction / ~1.7 MB cost to instantiate the
/// WASM VM, which would drown out entry-point regressions under a percentage
/// threshold. So [`OVERHEAD`] meters a trivial `health()` call on its own
/// (catching WASM-size bloat), and every other scenario is recorded *net* of
/// it — the work the entry point itself does.
fn measure_all(wasm: &[u8]) -> BTreeMap<StdString, Cost> {
    let h = Harness::new(wasm);
    let overhead = h.measure(|| {
        h.client.health();
    });
    let mut out = BTreeMap::from([(OVERHEAD.to_owned(), overhead)]);
    for sc in scenarios() {
        let h = Harness::new(wasm);
        let total = (sc.run)(&h);
        let net = Cost {
            cpu_insns: total.cpu_insns.saturating_sub(overhead.cpu_insns),
            mem_bytes: total.mem_bytes.saturating_sub(overhead.mem_bytes),
        };
        out.insert(sc.name.to_owned(), net);
    }
    out
}

// ─── Baseline file ───────────────────────────────────────────────────────────

/// Parsed `resource_baseline.toml`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Baseline {
    pub threshold_pct: u64,
    pub entries: BTreeMap<StdString, Cost>,
}

/// Parse the minimal TOML subset the baseline uses: a top-level
/// `threshold_pct`, then one `[scenario]` table per entry with integer
/// `cpu_insns` / `mem_bytes`. Underscore digit separators are accepted.
pub(crate) fn parse_baseline(src: &str) -> Result<Baseline, StdString> {
    let mut threshold_pct = DEFAULT_THRESHOLD_PCT;
    let mut entries: BTreeMap<StdString, (Option<u64>, Option<u64>)> = BTreeMap::new();
    let mut section: Option<StdString> = None;

    for (lineno, raw) in src.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let name = name.trim().to_owned();
            entries.entry(name.clone()).or_default();
            section = Some(name);
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("line {}: expected `key = value`", lineno + 1))?;
        let value: u64 =
            value.trim().replace('_', "").parse().map_err(|_| {
                format!("line {}: `{}` is not an integer", lineno + 1, value.trim())
            })?;
        match (section.as_deref(), key.trim()) {
            (None, "threshold_pct") => threshold_pct = value,
            (Some(s), "cpu_insns") => entries.get_mut(s).unwrap().0 = Some(value),
            (Some(s), "mem_bytes") => entries.get_mut(s).unwrap().1 = Some(value),
            (_, k) => return Err(format!("line {}: unknown key `{k}`", lineno + 1)),
        }
    }

    let mut out = BTreeMap::new();
    for (name, (cpu, mem)) in entries {
        match (cpu, mem) {
            (Some(cpu_insns), Some(mem_bytes)) => {
                out.insert(
                    name,
                    Cost {
                        cpu_insns,
                        mem_bytes,
                    },
                );
            }
            _ => return Err(format!("[{name}] needs both cpu_insns and mem_bytes")),
        }
    }
    Ok(Baseline {
        threshold_pct,
        entries: out,
    })
}

/// Render measurements as a baseline file (used by `--update`).
fn render_baseline(threshold_pct: u64, measured: &BTreeMap<StdString, Cost>) -> StdString {
    let mut s = StdString::from(
        "# Resource-usage baseline for the release WASM (#119).\n\
         #\n\
         # Generated by `scripts/check_resource_budget.sh --update`; do not edit\n\
         # numbers by hand. See COST_MODEL.md §12 for the update process.\n\n",
    );
    s.push_str(&format!("threshold_pct = {threshold_pct}\n"));
    for (name, c) in measured {
        s.push_str(&format!(
            "\n[{name}]\ncpu_insns = {}\nmem_bytes = {}\n",
            c.cpu_insns, c.mem_bytes
        ));
    }
    s
}

// ─── Comparison ──────────────────────────────────────────────────────────────

/// Outcome of comparing one metric against its baseline.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Finding {
    /// `measured` exceeds `baseline` by more than the threshold.
    Regressed {
        scenario: StdString,
        metric: &'static str,
        baseline: u64,
        measured: u64,
    },
    /// A scenario is metered but has no baseline entry.
    MissingBaseline { scenario: StdString },
    /// A baseline entry has no corresponding scenario (renamed/removed).
    StaleBaseline { scenario: StdString },
}

/// Signed percentage change from `base` to `now`, in tenths of a percent.
fn delta_permille(base: u64, now: u64) -> i128 {
    if base == 0 {
        return if now == 0 { 0 } else { i128::MAX };
    }
    (now as i128 - base as i128) * 1000 / base as i128
}

/// `true` when `now` is more than `threshold_pct` above `base`.
/// Exactly-at-threshold passes.
pub(crate) fn exceeds(base: u64, now: u64, threshold_pct: u64) -> bool {
    (now as u128) * 100 > (base as u128) * (100 + threshold_pct as u128)
}

/// Compare measurements against a baseline; empty result means the gate passes.
pub(crate) fn compare(
    baseline: &Baseline,
    measured: &BTreeMap<StdString, Cost>,
    threshold_pct: u64,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (name, now) in measured {
        let Some(base) = baseline.entries.get(name) else {
            findings.push(Finding::MissingBaseline {
                scenario: name.clone(),
            });
            continue;
        };
        for (metric, b, n) in [
            ("cpu_insns", base.cpu_insns, now.cpu_insns),
            ("mem_bytes", base.mem_bytes, now.mem_bytes),
        ] {
            if exceeds(b, n, threshold_pct) {
                findings.push(Finding::Regressed {
                    scenario: name.clone(),
                    metric,
                    baseline: b,
                    measured: n,
                });
            }
        }
    }
    for name in baseline.entries.keys() {
        if !measured.contains_key(name) {
            findings.push(Finding::StaleBaseline {
                scenario: name.clone(),
            });
        }
    }
    findings
}

fn fmt_pct(permille: i128) -> StdString {
    if permille == i128::MAX {
        return "+inf%".to_string();
    }
    let sign = if permille < 0 { "-" } else { "+" };
    let a = permille.unsigned_abs();
    format!("{sign}{}.{}%", a / 10, a % 10)
}

/// Human-readable, actionable description of a failing finding.
fn describe(f: &Finding, threshold_pct: u64) -> StdString {
    match f {
        Finding::Regressed {
            scenario,
            metric,
            baseline,
            measured,
        } => format!(
            "REGRESSION  {scenario}: {metric} {measured} vs baseline {baseline} \
             ({}; threshold +{threshold_pct}%)",
            fmt_pct(delta_permille(*baseline, *measured))
        ),
        Finding::MissingBaseline { scenario } => format!(
            "MISSING     {scenario}: no entry in the baseline — run \
             `scripts/check_resource_budget.sh --update` and commit the result"
        ),
        Finding::StaleBaseline { scenario } => format!(
            "STALE       {scenario}: baseline entry has no matching scenario — \
             re-run with --update to drop it"
        ),
    }
}

// ─── The gate ────────────────────────────────────────────────────────────────

/// Apply `SYNAPSE_RESOURCE_INJECT=scenario:pct` (CI self-test of the gate).
fn inject(measured: &mut BTreeMap<StdString, Cost>, spec: &str) {
    let (name, pct) = spec
        .split_once(':')
        .expect("SYNAPSE_RESOURCE_INJECT must be `scenario:pct`");
    let pct: u64 = pct.parse().expect("inject pct must be an integer");
    let c = measured
        .get_mut(name)
        .unwrap_or_else(|| panic!("SYNAPSE_RESOURCE_INJECT: unknown scenario `{name}`"));
    c.cpu_insns += c.cpu_insns * pct / 100;
    eprintln!("[resource] injected +{pct}% cpu_insns into `{name}` (gate self-test)");
}

#[test]
fn resource_gate_release_wasm() {
    if std::env::var("SYNAPSE_RESOURCE_GATE").as_deref() != Ok("1") {
        return; // Opt-in: driven by scripts/check_resource_budget.sh.
    }
    let wasm_path = std::env::var("SYNAPSE_RESOURCE_WASM")
        .expect("SYNAPSE_RESOURCE_WASM must point at the release WASM");
    let baseline_path = std::env::var("SYNAPSE_RESOURCE_BASELINE")
        .expect("SYNAPSE_RESOURCE_BASELINE must point at the baseline file");
    let wasm = std::fs::read(&wasm_path)
        .unwrap_or_else(|e| panic!("failed to read release WASM at {wasm_path}: {e}"));

    let mut measured = measure_all(&wasm);
    if let Ok(spec) = std::env::var("SYNAPSE_RESOURCE_INJECT") {
        inject(&mut measured, &spec);
    }

    let baseline_src = std::fs::read_to_string(&baseline_path).unwrap_or_default();
    let baseline = if baseline_src.is_empty() {
        Baseline {
            threshold_pct: DEFAULT_THRESHOLD_PCT,
            ..Default::default()
        }
    } else {
        parse_baseline(&baseline_src)
            .unwrap_or_else(|e| panic!("invalid baseline {baseline_path}: {e}"))
    };
    let threshold_pct = std::env::var("SYNAPSE_RESOURCE_THRESHOLD_PCT")
        .ok()
        .map(|v| v.parse().expect("threshold must be an integer"))
        .unwrap_or(baseline.threshold_pct);

    eprintln!(
        "[resource] {:<46} {:>12} {:>9} {:>10} {:>9}",
        "scenario", "cpu_insns", "Δcpu", "mem_bytes", "Δmem"
    );
    for (name, now) in &measured {
        let (dc, dm) = match baseline.entries.get(name) {
            Some(b) => (
                fmt_pct(delta_permille(b.cpu_insns, now.cpu_insns)),
                fmt_pct(delta_permille(b.mem_bytes, now.mem_bytes)),
            ),
            None => ("new".to_string(), "new".to_string()),
        };
        eprintln!(
            "[resource] {name:<46} {:>12} {dc:>9} {:>10} {dm:>9}",
            now.cpu_insns, now.mem_bytes
        );
    }

    if std::env::var("SYNAPSE_RESOURCE_UPDATE").as_deref() == Ok("1") {
        std::fs::write(&baseline_path, render_baseline(threshold_pct, &measured))
            .unwrap_or_else(|e| panic!("failed to write {baseline_path}: {e}"));
        eprintln!("[resource] baseline written to {baseline_path}");
        return;
    }

    let findings = compare(&baseline, &measured, threshold_pct);
    if !findings.is_empty() {
        let mut msg = format!(
            "\nResource-usage gate failed (#119): {} finding(s) against {baseline_path}\n\n",
            findings.len()
        );
        for f in &findings {
            msg.push_str("  ");
            msg.push_str(&describe(f, threshold_pct));
            msg.push('\n');
        }
        msg.push_str(
            "\nIf the increase is intended (e.g. a new feature genuinely needs it), \
             regenerate the baseline with\n  scripts/check_resource_budget.sh --update\n\
             and commit it with a justification in the PR (COST_MODEL.md §12).\n",
        );
        panic!("{msg}");
    }
    eprintln!("[resource] OK — all scenarios within +{threshold_pct}% of baseline");
}

// ─── Unit tests for the gate logic (always run) ──────────────────────────────

fn cost(cpu_insns: u64, mem_bytes: u64) -> Cost {
    Cost {
        cpu_insns,
        mem_bytes,
    }
}

fn one(name: &str, c: Cost) -> BTreeMap<StdString, Cost> {
    BTreeMap::from([(name.to_owned(), c)])
}

#[test]
fn gate_passes_within_and_exactly_at_threshold() {
    let b = Baseline {
        threshold_pct: 15,
        entries: one("x", cost(1000, 1000)),
    };
    assert!(compare(&b, &one("x", cost(1100, 900)), 15).is_empty());
    assert!(compare(&b, &one("x", cost(1150, 1150)), 15).is_empty());
}

#[test]
fn gate_fails_one_above_threshold_and_names_the_metric() {
    let b = Baseline {
        threshold_pct: 15,
        entries: one("register_callback", cost(1000, 1000)),
    };
    let f = compare(&b, &one("register_callback", cost(1151, 1000)), 15);
    assert_eq!(
        f,
        Vec::from([Finding::Regressed {
            scenario: "register_callback".to_owned(),
            metric: "cpu_insns",
            baseline: 1000,
            measured: 1151,
        }])
    );
    let msg = describe(&f[0], 15);
    assert!(
        msg.contains("register_callback") && msg.contains("+15.1%"),
        "{msg}"
    );
}

#[test]
fn gate_flags_missing_and_stale_entries() {
    let b = Baseline {
        threshold_pct: 15,
        entries: one("old", cost(1, 1)),
    };
    let f = compare(&b, &one("new", cost(1, 1)), 15);
    assert!(f.contains(&Finding::MissingBaseline {
        scenario: "new".to_owned()
    }));
    assert!(f.contains(&Finding::StaleBaseline {
        scenario: "old".to_owned()
    }));
}

#[test]
fn baseline_round_trips_through_render_and_parse() {
    let m = BTreeMap::from([
        ("a".to_owned(), cost(1_234_567, 89)),
        ("b".to_owned(), cost(0, 0)),
    ]);
    let parsed = parse_baseline(&render_baseline(12, &m)).unwrap();
    assert_eq!(parsed.threshold_pct, 12);
    assert_eq!(parsed.entries, m);
}

#[test]
fn baseline_parser_rejects_incomplete_entries() {
    assert!(parse_baseline("[x]\ncpu_insns = 1\n").is_err());
    assert!(parse_baseline("[x]\ncpu_insns = one\nmem_bytes = 1\n").is_err());
}

#[test]
fn committed_baseline_parses_and_covers_every_scenario() {
    let b = parse_baseline(include_str!("../resource_baseline.toml")).unwrap();
    let names: Vec<&str> = scenarios().iter().map(|s| s.name).collect();
    for name in names.iter().chain([&OVERHEAD]) {
        assert!(
            b.entries.contains_key(*name),
            "resource_baseline.toml has no entry for `{name}`"
        );
    }
    assert_eq!(b.entries.len(), names.len() + 1, "stale baseline entries");
}
