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
///
/// # Errors
///
/// Returns `Err` for any `ScVal` variant this renderer does not recognise,
/// carrying a description of the discriminant. Callers should propagate the
/// error so the failing test reports the unknown variant with context rather
/// than aborting the whole harness.
pub fn render_scval(v: &ScVal) -> Result<String, String> {
    match v {
        ScVal::Bool(b) => Ok(format!("{b}")),
        ScVal::Void => Ok("void".into()),
        ScVal::U32(n) => Ok(format!("u32:{n}")),
        ScVal::I32(n) => Ok(format!("i32:{n}")),
        ScVal::U64(n) => Ok(format!("u64:{n}")),
        ScVal::I64(n) => Ok(format!("i64:{n}")),
        ScVal::I128(parts) => {
            let n = ((parts.hi as i128) << 64) | parts.lo as i128;
            Ok(format!("i128:{n}"))
        }
        ScVal::U128(parts) => {
            let n = ((parts.hi as u128) << 64) | parts.lo as u128;
            Ok(format!("u128:{n}"))
        }
        ScVal::Symbol(s) => Ok(format!("sym:{}", s.0)),
        ScVal::String(s) => Ok(format!(
            "str:{:?}",
            std::string::String::from_utf8_lossy(&s.0)
        )),
        ScVal::Bytes(b) => Ok(format!("bytes:{}", hex(b.as_slice()))),
        ScVal::Address(a) => Ok(format!("addr:{a}")),
        ScVal::Vec(Some(items)) => {
            let inner: Vec<String> = items
                .iter()
                .map(render_scval)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("[{}]", inner.join(", ")))
        }
        ScVal::Vec(None) => Ok("[]".into()),
        ScVal::Map(Some(entries)) => {
            let inner: Vec<String> = entries
                .iter()
                .map(|e| {
                    let key = render_map_key(&e.key)?;
                    let val = render_scval(&e.val)?;
                    Ok(format!("{key}: {val}"))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(format!("{{{}}}", inner.join(", ")))
        }
        ScVal::Map(None) => Ok("{}".into()),
        other => Err(format!(
            "render_scval: unrecognised ScVal variant: {other:?}"
        )),
    }
}

fn render_map_key(k: &ScVal) -> Result<String, String> {
    match k {
        ScVal::Symbol(s) => Ok(format!("{}", s.0)),
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
/// # Panics
///
/// Panics if any event topic or data is an `ScVal` variant that
/// [`render_scval`] does not recognise. The panic message names the
/// unrecognised variant so the failing test has actionable context.
pub fn render_last_events(env: &Env) -> Vec<String> {
    let mut out = Vec::new();
    for (_contract, topics, data) in env.events().all().iter() {
        let topic_vals: Vec<ScVal> = topics.iter().map(|t| to_scval(env, &t)).collect();
        let topics_sc = ScVal::Vec(Some(
            topic_vals
                .clone()
                .try_into()
                .expect("topic count fits ScVec"),
        ));
        let data_sc = to_scval(env, &data);
        let rendered_topics: Vec<String> = topic_vals
            .iter()
            .map(|v| render_scval(v).expect("event topic contains unrecognised ScVal variant"))
            .collect();
        out.push(format!("topics: [{}]", rendered_topics.join(", ")));
        out.push(format!(
            "data:   {}",
            render_scval(&data_sc)
                .expect("event data contains unrecognised ScVal variant")
        ));
        out.push(format!(
            "xdr:    {}|{}",
            xdr_hex(&topics_sc),
            xdr_hex(&data_sc)
        ));
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::xdr::ScVal;

    // ── render_scval known-variant round-trips ────────────────────────────────

    #[test]
    fn render_bool() {
        assert_eq!(render_scval(&ScVal::Bool(true)), Ok("true".into()));
        assert_eq!(render_scval(&ScVal::Bool(false)), Ok("false".into()));
    }

    #[test]
    fn render_void() {
        assert_eq!(render_scval(&ScVal::Void), Ok("void".into()));
    }

    #[test]
    fn render_u32() {
        assert_eq!(render_scval(&ScVal::U32(42)), Ok("u32:42".into()));
    }

    #[test]
    fn render_i64() {
        assert_eq!(render_scval(&ScVal::I64(-7)), Ok("i64:-7".into()));
    }

    // ── render_scval unknown-variant returns Err ──────────────────────────────

    /// `ScVal::LedgerKeyContractInstance` is a valid XDR discriminant that the
    /// contract never emits. Feeding it to `render_scval` must return `Err`
    /// with a descriptive message rather than aborting the harness.
    #[test]
    fn render_scval_unknown_variant_returns_err() {
        let unknown = ScVal::LedgerKeyContractInstance;
        let result = render_scval(&unknown);
        assert!(
            result.is_err(),
            "expected Err for unrecognised variant, got Ok({:?})",
            result
        );
        let msg = result.unwrap_err();
        assert!(
            msg.contains("unrecognised ScVal variant"),
            "error message should describe the variant, got: {msg:?}"
        );
    }
}
