//! # Shared test rendering helpers
//!
//! Deterministic, human-diffable text renderings of the contract's
//! *observable* behaviour — emitted events and persisted ledger state — used
//! by:
//!
//! * [`crate::test_event_snapshots`] (#133) to pin exact event payloads
//!   against committed fixtures, and
//! * [`crate::test_profile_diff`] (#131) to compare the same scenario across
//!   the `release` and `release-with-logs` build profiles.
//!
//! Every rendering carries two views of the same value: a readable form (for
//! reviewing diffs) and the raw XDR bytes as hex (so a byte-level change that
//! the readable form happens to hide still fails the comparison).
//!
//! Nothing here may depend on the build profile (e.g. `cfg!(debug_assertions)`)
//! or on host-side randomness: the renderings must be byte-identical for
//! identical contract behaviour.

#![cfg(test)]

extern crate std;

use std::{format, string::String, vec::Vec};

use soroban_sdk::{
    testutils::Events,
    xdr::{Limits, ScVal, WriteXdr},
    Env, TryFromVal, Val,
};

/// Render a single [`ScVal`] in a compact, stable, readable form.
///
/// Struct payloads (`#[contracttype]` structs encode as `ScVal::Map` with
/// symbol keys) render as `{field: value, …}` in wire order; unit enum
/// variants (`ScVal::Vec([Symbol])`) render as `[Variant]`.
pub fn render_scval(v: &ScVal) -> String {
    match v {
        ScVal::Bool(b) => format!("{b}"),
        ScVal::Void => "void".into(),
        ScVal::U32(n) => format!("u32:{n}"),
        ScVal::I32(n) => format!("i32:{n}"),
        ScVal::U64(n) => format!("u64:{n}"),
        ScVal::I64(n) => format!("i64:{n}"),
        ScVal::I128(parts) => {
            let n = ((parts.hi as i128) << 64) | parts.lo as i128;
            format!("i128:{n}")
        }
        ScVal::U128(parts) => {
            let n = ((parts.hi as u128) << 64) | parts.lo as u128;
            format!("u128:{n}")
        }
        ScVal::Symbol(s) => format!("sym:{}", s.0),
        ScVal::String(s) => format!("str:{:?}", std::string::String::from_utf8_lossy(&s.0)),
        ScVal::Bytes(b) => format!("bytes:{}", hex(b.as_slice())),
        ScVal::Address(a) => format!("addr:{a}"),
        ScVal::Vec(Some(items)) => {
            let inner: Vec<String> = items.iter().map(render_scval).collect();
            format!("[{}]", inner.join(", "))
        }
        ScVal::Map(Some(entries)) => {
            let inner: Vec<String> = entries
                .iter()
                .map(|e| format!("{}: {}", render_map_key(&e.key), render_scval(&e.val)))
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        // Anything the contract does not currently emit falls back to the
        // XDR type's own Debug impl — still deterministic, just less terse.
        other => format!("{other:?}"),
    }
}

fn render_map_key(k: &ScVal) -> String {
    match k {
        ScVal::Symbol(s) => format!("{}", s.0),
        other => render_scval(other),
    }
}

/// Lower-case hex encoding.
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// XDR-encode a value and hex it.
pub fn xdr_hex<T: WriteXdr>(v: &T) -> String {
    hex(&v
        .to_xdr(Limits::none())
        .expect("XDR encoding cannot fail for host values"))
}

fn to_scval(env: &Env, v: &Val) -> ScVal {
    ScVal::try_from_val(env, v).expect("every emitted Val converts to ScVal")
}

/// Render every contract event published by the **most recent** top-level
/// invocation, in emission order.
///
/// Each event produces three lines:
///
/// ```text
/// topics: [sym:synapse, sym:reg]
/// data:   {tx_id: str:"tx-1", …}
/// xdr:    <hex(topics ScVal::Vec)>|<hex(data ScVal)>
/// ```
///
/// The emitting contract id is intentionally omitted: it is an artefact of
/// test registration order, not of the payload shape subscribers depend on.
///
/// ## Incremental rendering
///
/// Each call records how many events it rendered via a thread-local cache
/// keyed on the number of events seen. On subsequent calls against the same
/// growing buffer the already-rendered lines are returned from the cache and
/// only the newly appended events are rendered. Output is byte-identical to a
/// full re-render from scratch, so existing snapshot fixtures do not need
/// regeneration.
pub fn render_last_events(env: &Env) -> Vec<String> {
    use std::{cell::RefCell, collections::HashMap};

    // Maps (total_event_count_at_last_render) → cached rendered lines for
    // the **entire** buffer up to that point.
    //
    // We use the event count as the cache key because a Soroban test `Env`
    // is single-threaded and the event buffer only grows.  Two distinct `Env`
    // instances will naturally produce independent counts, and within one
    // `Env` a cache miss simply means we extend the existing lines.
    thread_local! {
        // (last_seen_len, accumulated_lines)
        static CACHE: RefCell<HashMap<usize, Vec<String>>> = RefCell::new(HashMap::new());
    }

    let all_events = env.events().all();
    let total = all_events.len() as usize;

    if total == 0 {
        return Vec::new();
    }

    CACHE.with(|cell| {
        let mut map = cell.borrow_mut();

        // Find the largest cached prefix that is ≤ total.
        let (start, mut lines) = {
            let best = map
                .iter()
                .filter(|(&k, _)| k <= total)
                .max_by_key(|(&k, _)| k);
            match best {
                Some((&k, v)) => (k, v.clone()),
                None => (0, Vec::new()),
            }
        };

        // Render only the events not yet in the cache.
        for (_contract, topics, data) in all_events.iter().skip(start) {
            let topic_vals: Vec<ScVal> = topics.iter().map(|t| to_scval(env, &t)).collect();
            let topics_sc = ScVal::Vec(Some(
                topic_vals
                    .clone()
                    .try_into()
                    .expect("topic count fits ScVec"),
            ));
            let data_sc = to_scval(env, &data);
            let rendered_topics: Vec<String> = topic_vals.iter().map(render_scval).collect();
            lines.push(format!("topics: [{}]", rendered_topics.join(", ")));
            lines.push(format!("data:   {}", render_scval(&data_sc)));
            lines.push(format!(
                "xdr:    {}|{}",
                xdr_hex(&topics_sc),
                xdr_hex(&data_sc)
            ));
        }

        map.insert(total, lines.clone());
        lines
    })
}

/// Render every ledger entry currently in the test host's storage, sorted by
/// the XDR encoding of its key so the order is independent of host map
/// iteration order.
///
/// Each entry renders as `key-xdr => entry-xdr ttl=<live_until>`: exact bytes,
/// so any divergence in stored state — including a field that no event or
/// return value exposes — shows up in a comparison.
pub fn render_ledger_state(env: &Env) -> Vec<String> {
    let snapshot = env.to_ledger_snapshot();
    let mut rows: Vec<String> = snapshot
        .ledger_entries
        .iter()
        .map(|(key, (entry, live_until))| {
            let ttl = match live_until {
                Some(l) => format!("{l}"),
                None => "none".into(),
            };
            format!(
                "{} => {} ttl={}",
                xdr_hex(key.as_ref()),
                xdr_hex(&entry.data),
                ttl
            )
        })
        .collect();
    rows.sort();
    rows
}

/// 64-bit FNV-1a. Used for compact per-step state digests; hand-rolled so the
/// output is stable across toolchains and build profiles (unlike
/// `std::hash::DefaultHasher`, whose algorithm is unspecified).
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Line-oriented diff between an expected and actual rendering, marking each
/// differing line with `-` (expected) / `+` (actual). Returns `None` when the
/// two are identical.
pub fn line_diff(expected: &str, actual: &str) -> Option<String> {
    if expected == actual {
        return None;
    }
    let exp: Vec<&str> = expected.lines().collect();
    let act: Vec<&str> = actual.lines().collect();
    let mut out = String::new();
    for i in 0..exp.len().max(act.len()) {
        match (exp.get(i), act.get(i)) {
            (Some(e), Some(a)) if e == a => out.push_str(&format!("  {e}\n")),
            (e, a) => {
                if let Some(e) = e {
                    out.push_str(&format!("- {e}\n"));
                }
                if let Some(a) = a {
                    out.push_str(&format!("+ {a}\n"));
                }
            }
        }
    }
    Some(out)
}
