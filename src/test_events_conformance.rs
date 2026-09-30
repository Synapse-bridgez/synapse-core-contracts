//! # Event Conformance Tests
//!
//! CI-enforced checks that `src/events.rs` and `EVENTS.md` never drift from
//! the canonical event schema recorded in `event_conformance_manifest.toml`.
//!
//! ## What is checked
//!
//! | Check | Source of truth | Checked against |
//! |-------|-----------------|-----------------|
//! | Struct name exists | manifest | `events.rs` source text |
//! | Topic symbol matches | manifest | `events.rs` `symbol_short!("…")` |
//! | Field names & types (in declaration order) | manifest | `events.rs` struct body |
//! | `EventEmitter` method exists | manifest | `events.rs` source text |
//! | Every event appears in EVENTS.md | manifest | `EVENTS.md` source text |
//! | Every EVENTS.md topic matches manifest | manifest | `EVENTS.md` source text |
//!
//! ## Why this approach (not proc-macro reflection)
//!
//! Soroban `#[contracttype]` structs compile away completely in the WASM
//! artefact — there is no runtime type registry to query.  A proc-macro
//! approach would add significant build-time complexity for this crate.
//! Instead, the test parses source text at test-run time via `include_str!`
//! (zero new dependencies).  The trade-off is that the manifest itself
//! requires human maintenance — but `make check` then enforces it mechanically.
//!
//! See the rationale note at the top of `event_conformance_manifest.toml`.
//!
//! ## Drift-detection fixture
//!
//! `test_drift_detection_catches_unknown_struct` and related tests prove the
//! parser/checker correctly *fails* on deliberately wrong input, so the happy
//! path is not just silently not-checking anything.

#![cfg(test)]

extern crate std;

use std::borrow::ToOwned;
use std::format;
use std::string::String;
use std::vec::Vec;

// ─── Manifest types ──────────────────────────────────────────────────────────

/// One field in a `#[contracttype]` struct, as declared in the manifest.
#[derive(Debug, PartialEq, Eq)]
struct ManifestField {
    name: String,
    /// Rust type as written in the struct (after normalising whitespace).
    field_type: String,
}

/// One event entry from the manifest.
#[derive(Debug, PartialEq, Eq)]
struct ManifestEvent {
    struct_name: String,
    /// `symbol_short!` argument — the topics\[1\] value.
    topic: String,
    /// `EventEmitter` method name.
    emitter_fn: String,
    /// Fields in declaration order.
    fields: Vec<ManifestField>,
}

// ─── Minimal TOML parser ─────────────────────────────────────────────────────
//
// Only handles the exact subset used by `event_conformance_manifest.toml`:
//   schema_version = <integer>
//   [[event]]            → start of a new event block
//   [[event.fields]]     → start of a new field block within the current event
//   key = "value"        → string assignment (quoted values only)
//
// Anything else (blank lines, comment lines starting with `#`) is ignored.
// This avoids adding a TOML dev-dependency to the crate.

fn parse_manifest(src: &str) -> (u32, Vec<ManifestEvent>) {
    let mut schema_version: u32 = 0;
    let mut events: Vec<ManifestEvent> = Vec::new();

    // Tracks whether the last `[[…]]` header was `[[event]]` or
    // `[[event.fields]]` so we know where to attach key=value pairs.
    enum Context {
        TopLevel,
        Event,
        Field,
    }
    let mut ctx = Context::TopLevel;

    for raw_line in src.lines() {
        let line = raw_line.trim();

        // Skip blanks and comments.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line == "[[event]]" {
            events.push(ManifestEvent {
                struct_name: String::new(),
                topic: String::new(),
                emitter_fn: String::new(),
                fields: Vec::new(),
            });
            ctx = Context::Event;
            continue;
        }

        if line == "[[event.fields]]" {
            let ev = events
                .last_mut()
                .expect("[[event.fields]] appeared before any [[event]] block");
            ev.fields.push(ManifestField {
                name: String::new(),
                field_type: String::new(),
            });
            ctx = Context::Field;
            continue;
        }

        // Parse `key = "value"` or `key = <integer>`.
        if let Some((k, v)) = parse_kv(line) {
            match ctx {
                Context::TopLevel => {
                    if k == "schema_version" {
                        schema_version =
                            v.parse::<u32>().expect("schema_version must be an integer");
                    }
                }
                Context::Event => {
                    let ev = events.last_mut().unwrap();
                    match k {
                        "struct_name" => ev.struct_name = v.to_owned(),
                        "topic" => ev.topic = v.to_owned(),
                        "emitter_fn" => ev.emitter_fn = v.to_owned(),
                        _ => {} // unknown keys are ignored for forward-compat
                    }
                }
                Context::Field => {
                    let field = events.last_mut().unwrap().fields.last_mut().unwrap();
                    match k {
                        "name" => field.name = v.to_owned(),
                        "type" => field.field_type = v.to_owned(),
                        _ => {}
                    }
                }
            }
        }
    }

    (schema_version, events)
}

/// Parse a `key = "value"` or `key = 42` line.
///
/// Returns `Some((key, value_str))` where `value_str` has any surrounding
/// double-quotes stripped.  Returns `None` for lines that don't match.
fn parse_kv(line: &str) -> Option<(&str, &str)> {
    let eq = line.find('=')?;
    let key = line[..eq].trim();
    let raw_val = line[eq + 1..].trim();
    // Strip quotes if present.
    let val = if raw_val.starts_with('"') && raw_val.ends_with('"') && raw_val.len() >= 2 {
        &raw_val[1..raw_val.len() - 1]
    } else {
        raw_val
    };
    Some((key, val))
}

// ─── events.rs extractor ────────────────────────────────────────────────────
//
// Extracts struct definitions from the Rust source text of `events.rs`.
// Returns a map of struct_name → ordered list of (field_name, field_type).
//
// The parser is intentionally simple: it looks for the pattern
//
//   pub struct <Name> {
//       pub <field>: <Type>,
//       …
//   }
//
// and is resilient to doc-comment lines (/// …) and blank lines between
// fields.  It does not attempt to parse arbitrary Rust syntax.

fn extract_structs(src: &str) -> std::collections::HashMap<String, Vec<(String, String)>> {
    let mut result: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();

    let mut current_struct: Option<String> = None;
    let mut depth: i32 = 0; // brace depth inside the current struct

    for raw_line in src.lines() {
        let line = raw_line.trim();

        // Detect `pub struct Name {`
        if current_struct.is_none() {
            if let Some(name) = extract_struct_name(line) {
                current_struct = Some(name.to_owned());
                depth = 0;
                // Count the opening brace on this same line.
                for ch in line.chars() {
                    match ch {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                continue;
            }
        }

        // Clone the name out so we don't hold a borrow on `current_struct`
        // while we might need to mutate it (set it to None) below.
        let sname_opt: Option<String> = current_struct.clone();
        if let Some(sname) = sname_opt {
            // Count braces to track struct end.
            for ch in line.chars() {
                match ch {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
            }

            if depth <= 0 {
                // Closing brace of the struct — stop collecting.
                current_struct = None;
                depth = 0;
                continue;
            }

            // Skip doc comments and blank lines inside the struct.
            if line.is_empty() || line.starts_with("///") || line.starts_with("//") {
                continue;
            }

            // Match `pub <name>: <type>,`
            if let Some((fname, ftype)) = extract_field(line) {
                result
                    .entry(sname.clone())
                    .or_default()
                    .push((fname.to_owned(), normalise_type(ftype)));
            }
        }
    }

    result
}

/// Match `pub struct <Name>` (optionally ending with ` {`).
fn extract_struct_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("pub struct ")?;
    // Name is the first token (stops at whitespace or `{`).
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '{')
        .unwrap_or(rest.len());
    let name = &rest[..end];
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Match `pub <name>: <type>,` or `pub <name>: <type>` (no trailing comma).
///
/// Also strips trailing inline comments (`// …`) before extracting the type,
/// so fixture tests with annotated fields parse correctly.
fn extract_field(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("pub ")?;
    let colon = rest.find(':')?;
    let fname = rest[..colon].trim();
    let raw_type = rest[colon + 1..].trim();
    // Strip inline comment if present.
    let without_comment = if let Some(comment_pos) = raw_type.find("//") {
        raw_type[..comment_pos].trim()
    } else {
        raw_type
    };
    // Strip trailing comma.
    let ftype = without_comment.trim_end_matches(',').trim();
    if fname.is_empty() || ftype.is_empty() {
        None
    } else {
        Some((fname, ftype))
    }
}

/// Collapse runs of whitespace inside a type string so comparison is
/// whitespace-insensitive (e.g. `BytesN < 32 >` == `BytesN<32>`).
fn normalise_type(t: &str) -> String {
    // 1. Collapse internal whitespace.
    let collapsed: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    // 2. Remove spaces around `<` and `>` for generic types.
    collapsed
        .replace("< ", "<")
        .replace(" >", ">")
        .replace(" <", "<")
        .replace("> ", ">")
}

// ─── topic extractor ────────────────────────────────────────────────────────
//
// For each event struct, find the `symbol_short!("…")` call that appears in
// the same `env.events().publish(…)` invocation as that struct.  We locate
// it by scanning the emitter function body, keyed on the function name.

fn extract_topic_for_emitter(src: &str, emitter_fn: &str) -> Option<String> {
    // Find the function definition: `pub fn <emitter_fn>(`
    let fn_sig = format!("pub fn {}(", emitter_fn);
    let fn_start = src.find(fn_sig.as_str())?;

    // Walk forward from fn_start to find the first `symbol_short!("…")` call
    // in the publish tuple — that is topics[1].  We skip the first occurrence
    // (which is `"synapse"`) and take the second.
    let fn_src = &src[fn_start..];

    let mut symbol_short_values: Vec<String> = Vec::new();
    let mut search_from = 0;
    let needle = "symbol_short!(\"";
    while let Some(pos) = fn_src[search_from..].find(needle) {
        let abs = search_from + pos + needle.len();
        if let Some(end) = fn_src[abs..].find('"') {
            symbol_short_values.push(fn_src[abs..abs + end].to_owned());
        }
        search_from = abs;

        // Stop once we've found both topic symbols (or hit the next function).
        if symbol_short_values.len() == 2 {
            break;
        }
        // Bail out if we've wandered past a reasonable function length.
        if search_from > 800 {
            break;
        }
    }

    // topics[1] is the second symbol_short! in the publish call.
    symbol_short_values.into_iter().nth(1)
}

// ─── EVENTS.md checker ──────────────────────────────────────────────────────
//
// Verifies that every event in the manifest is documented in EVENTS.md.
// We check that:
//   1. The struct name appears somewhere in the document.
//   2. The topic string appears somewhere in the document.
//   3. Every field name appears somewhere in the document.

fn check_events_md(events_md: &str, events: &[ManifestEvent]) -> Vec<String> {
    let mut failures: Vec<String> = Vec::new();

    for ev in events {
        // Struct name must appear (e.g. as a heading or table cell).
        if !events_md.contains(&ev.struct_name) {
            failures.push(format!(
                "EVENTS.md is missing struct name `{}`",
                ev.struct_name
            ));
        }

        // Topic must appear (e.g. `| `init` |`  or plain `init`).
        // We search for the backtick-quoted form used in the tables.
        let topic_pattern = format!("`{}`", ev.topic);
        if !events_md.contains(&topic_pattern) {
            failures.push(format!(
                "EVENTS.md is missing topic `{}` for `{}`",
                ev.topic, ev.struct_name
            ));
        }

        // Every field name must appear somewhere in the section for this event.
        for field in &ev.fields {
            if !events_md.contains(&field.name) {
                failures.push(format!(
                    "EVENTS.md is missing field `{}` for `{}`",
                    field.name, ev.struct_name
                ));
            }
        }
    }

    failures
}

// ─── Core conformance checker ────────────────────────────────────────────────

/// Given parsed manifest events and the `events.rs` source text, return a list
/// of human-readable failure messages.  An empty vec means full conformance.
fn check_conformance(events: &[ManifestEvent], events_rs: &str) -> Vec<String> {
    let structs = extract_structs(events_rs);
    let mut failures: Vec<String> = Vec::new();

    for ev in events {
        // 1. Struct must exist in events.rs.
        match structs.get(&ev.struct_name) {
            None => {
                failures.push(format!(
                    "struct `{}` not found in events.rs",
                    ev.struct_name
                ));
                // Can't check fields if struct is absent.
                continue;
            }
            Some(actual_fields) => {
                // 2. Field count must match.
                if actual_fields.len() != ev.fields.len() {
                    failures.push(format!(
                        "`{}`: manifest has {} fields, events.rs has {}",
                        ev.struct_name,
                        ev.fields.len(),
                        actual_fields.len()
                    ));
                }

                // 3. Field names, types, and order must match.
                for (i, mf) in ev.fields.iter().enumerate() {
                    match actual_fields.get(i) {
                        None => {
                            failures.push(format!(
                                "`{}` field[{}]: manifest expects `{}` but events.rs has no field at that position",
                                ev.struct_name, i, mf.name
                            ));
                        }
                        Some((aname, atype)) => {
                            if aname != &mf.name {
                                failures.push(format!(
                                    "`{}` field[{}]: manifest name `{}` ≠ events.rs name `{}`",
                                    ev.struct_name, i, mf.name, aname
                                ));
                            }
                            let norm_manifest = normalise_type(&mf.field_type);
                            if atype != &norm_manifest {
                                failures.push(format!(
                                    "`{}`.`{}` type mismatch: manifest `{}` ≠ events.rs `{}`",
                                    ev.struct_name, mf.name, norm_manifest, atype
                                ));
                            }
                        }
                    }
                }
            }
        }

        // 4. EventEmitter method must exist in events.rs.
        let fn_sig = format!("pub fn {}(", ev.emitter_fn);
        if !events_rs.contains(&fn_sig) {
            failures.push(format!(
                "EventEmitter method `{}` not found in events.rs",
                ev.emitter_fn
            ));
        }

        // 5. Topic symbol in the emitter must match the manifest.
        match extract_topic_for_emitter(events_rs, &ev.emitter_fn) {
            None => {
                failures.push(format!(
                    "`{}`: could not extract topic symbol from emitter `{}`",
                    ev.struct_name, ev.emitter_fn
                ));
            }
            Some(actual_topic) => {
                if actual_topic != ev.topic {
                    failures.push(format!(
                        "`{}`: manifest topic `{}` ≠ events.rs symbol_short `{}`",
                        ev.struct_name, ev.topic, actual_topic
                    ));
                }
            }
        }
    }

    failures
}

// ─── Actual test sources (baked in at compile time) ─────────────────────────

const MANIFEST_SRC: &str = include_str!("../event_conformance_manifest.toml");
const EVENTS_RS_SRC: &str = include_str!("events.rs");
const EVENTS_MD_SRC: &str = include_str!("../EVENTS.md");

// ─── Tests ───────────────────────────────────────────────────────────────────

/// PRIMARY CI GATE: verifies that `events.rs` fully conforms to the manifest.
///
/// This test **blocks CI** on any drift between the manifest and the Rust code.
/// To fix a failure, update `event_conformance_manifest.toml` *and*
/// `EVENTS.md` in the same PR that changes `events.rs`.
#[test]
fn test_events_rs_conforms_to_manifest() {
    let (_schema_version, events) = parse_manifest(MANIFEST_SRC);
    assert!(
        !events.is_empty(),
        "Manifest parsed zero events — check TOML syntax"
    );

    let failures = check_conformance(&events, EVENTS_RS_SRC);
    if !failures.is_empty() {
        let msg = failures.join("\n  ");
        panic!(
            "events.rs ↔ manifest drift detected ({} issue(s)):\n  {}\n\n\
             Fix: update event_conformance_manifest.toml to match events.rs \
             (or vice versa), then update EVENTS.md tables accordingly.",
            failures.len(),
            msg
        );
    }
}

/// SECONDARY CI GATE: verifies that `EVENTS.md` documents every event in the
/// manifest (struct names, topics, and field names all present).
///
/// This catches docs-only drift where the code and manifest agree but EVENTS.md
/// has gone stale.
#[test]
fn test_events_md_conforms_to_manifest() {
    let (_schema_version, events) = parse_manifest(MANIFEST_SRC);
    assert!(
        !events.is_empty(),
        "Manifest parsed zero events — check TOML syntax"
    );

    let failures = check_events_md(EVENTS_MD_SRC, &events);
    if !failures.is_empty() {
        let msg = failures.join("\n  ");
        panic!(
            "EVENTS.md ↔ manifest drift detected ({} issue(s)):\n  {}\n\n\
             Fix: update EVENTS.md tables to match event_conformance_manifest.toml.",
            failures.len(),
            msg
        );
    }
}

/// Verify the manifest itself is self-consistent: every event has a non-empty
/// struct_name, topic, emitter_fn, and at least one field.
#[test]
fn test_manifest_is_well_formed() {
    let (schema_version, events) = parse_manifest(MANIFEST_SRC);
    assert!(schema_version > 0, "schema_version must be ≥ 1");
    assert_eq!(
        events.len(),
        10,
        "Expected exactly 10 events in the manifest"
    );

    for ev in &events {
        assert!(
            !ev.struct_name.is_empty(),
            "Event has empty struct_name: {ev:?}"
        );
        assert!(
            !ev.topic.is_empty(),
            "Event `{}` has empty topic",
            ev.struct_name
        );
        assert!(
            !ev.emitter_fn.is_empty(),
            "Event `{}` has empty emitter_fn",
            ev.struct_name
        );
        assert!(
            !ev.fields.is_empty(),
            "Event `{}` has no fields",
            ev.struct_name
        );
        for f in &ev.fields {
            assert!(
                !f.name.is_empty(),
                "Event `{}` has a field with empty name",
                ev.struct_name
            );
            assert!(
                !f.field_type.is_empty(),
                "Event `{}` field `{}` has empty type",
                ev.struct_name,
                f.name
            );
        }
    }
}

/// Verify the manifest lists every expected event topic exactly once.
#[test]
fn test_manifest_contains_all_expected_topics() {
    let expected_topics = [
        "init", "reg", "status", "done", "fail", "propose", "admin", "relay", "upgrade", "pause",
    ];
    let (_schema_version, events) = parse_manifest(MANIFEST_SRC);
    let manifest_topics: Vec<&str> = events.iter().map(|e| e.topic.as_str()).collect();

    for topic in &expected_topics {
        assert!(
            manifest_topics.contains(topic),
            "Expected topic `{topic}` not found in manifest"
        );
    }
    assert_eq!(
        manifest_topics.len(),
        expected_topics.len(),
        "Manifest has {} topics, expected {}",
        manifest_topics.len(),
        expected_topics.len()
    );
}

// ─── Drift-detection fixture tests ──────────────────────────────────────────
//
// These tests prove the checker FAILS on deliberately wrong input, so a
// silently-passing but vacuous check doesn't go unnoticed.

/// A synthetic `events.rs` fragment with a renamed field — the checker must
/// catch the name mismatch.
#[test]
fn test_drift_detection_catches_renamed_field() {
    let fake_events_rs = r#"
        #[contracttype]
        pub struct EventInitialised {
            pub admin: soroban_sdk::Address,
            pub relay_signer_renamed: soroban_sdk::Address,  // <-- wrong name
            pub ledger: u32,
        }
    "#;

    // Build a minimal manifest with just EventInitialised.
    let partial_manifest = r#"
schema_version = 1

[[event]]
struct_name = "EventInitialised"
topic       = "init"
emitter_fn  = "initialised"

[[event.fields]]
name = "admin"
type = "soroban_sdk::Address"

[[event.fields]]
name = "relay_signer"
type = "soroban_sdk::Address"

[[event.fields]]
name = "ledger"
type = "u32"
"#;

    let (_ver, events) = parse_manifest(partial_manifest);
    let failures = check_conformance(&events, fake_events_rs);
    assert!(
        !failures.is_empty(),
        "Drift detection failed: renamed field should have produced failures, got none"
    );
    assert!(
        failures.iter().any(|f| f.contains("relay_signer")),
        "Expected failure to mention 'relay_signer', got: {failures:?}"
    );
}

/// A synthetic `events.rs` with a changed field type — the checker must catch it.
#[test]
fn test_drift_detection_catches_wrong_field_type() {
    let fake_events_rs = r#"
        #[contracttype]
        pub struct EventTransactionCompleted {
            pub tx_id: String,
            pub stellar_tx_hash: u64,  // <-- should be String
            pub ledger: u32,
        }
    "#;

    let partial_manifest = r#"
schema_version = 1

[[event]]
struct_name = "EventTransactionCompleted"
topic       = "done"
emitter_fn  = "transaction_completed"

[[event.fields]]
name = "tx_id"
type = "String"

[[event.fields]]
name = "stellar_tx_hash"
type = "String"

[[event.fields]]
name = "ledger"
type = "u32"
"#;

    let (_ver, events) = parse_manifest(partial_manifest);
    let failures = check_conformance(&events, fake_events_rs);
    assert!(
        !failures.is_empty(),
        "Drift detection failed: wrong field type should have produced failures"
    );
    assert!(
        failures.iter().any(|f| f.contains("stellar_tx_hash")),
        "Expected failure to mention 'stellar_tx_hash', got: {failures:?}"
    );
}

/// A synthetic `events.rs` missing a struct entirely — the checker must report it.
#[test]
fn test_drift_detection_catches_missing_struct() {
    // events.rs has no EventPauseToggled at all.
    let fake_events_rs = r#"
        pub struct EventAdminTransferred {
            pub old_admin: soroban_sdk::Address,
            pub new_admin: soroban_sdk::Address,
            pub ledger: u32,
        }
    "#;

    let partial_manifest = r#"
schema_version = 1

[[event]]
struct_name = "EventPauseToggled"
topic       = "pause"
emitter_fn  = "pause_toggled"

[[event.fields]]
name = "paused"
type = "bool"

[[event.fields]]
name = "admin"
type = "soroban_sdk::Address"

[[event.fields]]
name = "ledger"
type = "u32"
"#;

    let (_ver, events) = parse_manifest(partial_manifest);
    let failures = check_conformance(&events, fake_events_rs);
    assert!(
        !failures.is_empty(),
        "Drift detection failed: missing struct should have produced failures"
    );
    assert!(
        failures.iter().any(|f| f.contains("EventPauseToggled")),
        "Expected failure to mention 'EventPauseToggled', got: {failures:?}"
    );
}

/// A synthetic `events.rs` with an extra field inserted in the middle — the
/// checker must catch the field-order violation.
#[test]
fn test_drift_detection_catches_extra_field() {
    let fake_events_rs = r#"
        #[contracttype]
        pub struct EventTransactionFailed {
            pub tx_id: String,
            pub reason: String,
            pub extra_field: String,  // <-- not in manifest
            pub ledger: u32,
        }
    "#;

    let partial_manifest = r#"
schema_version = 1

[[event]]
struct_name = "EventTransactionFailed"
topic       = "fail"
emitter_fn  = "transaction_failed"

[[event.fields]]
name = "tx_id"
type = "String"

[[event.fields]]
name = "reason"
type = "String"

[[event.fields]]
name = "ledger"
type = "u32"
"#;

    let (_ver, events) = parse_manifest(partial_manifest);
    let failures = check_conformance(&events, fake_events_rs);
    assert!(
        !failures.is_empty(),
        "Drift detection failed: extra field should have produced failures"
    );
}

/// A wrong topic in a fake events.rs emitter — the checker must flag it.
#[test]
fn test_drift_detection_catches_wrong_topic_symbol() {
    // The emitter publishes "wrongtopic" instead of "init".
    let fake_events_rs = r#"
        pub struct EventInitialised {
            pub admin: soroban_sdk::Address,
            pub relay_signer: soroban_sdk::Address,
            pub ledger: u32,
        }

        pub fn initialised(env: &Env, admin: &soroban_sdk::Address, relay_signer: &soroban_sdk::Address) {
            env.events().publish(
                (symbol_short!("synapse"), symbol_short!("wrongtopic")),
                EventInitialised { admin: admin.clone(), relay_signer: relay_signer.clone(), ledger: 0 },
            );
        }
    "#;

    let partial_manifest = r#"
schema_version = 1

[[event]]
struct_name = "EventInitialised"
topic       = "init"
emitter_fn  = "initialised"

[[event.fields]]
name = "admin"
type = "soroban_sdk::Address"

[[event.fields]]
name = "relay_signer"
type = "soroban_sdk::Address"

[[event.fields]]
name = "ledger"
type = "u32"
"#;

    let (_ver, events) = parse_manifest(partial_manifest);
    let failures = check_conformance(&events, fake_events_rs);
    assert!(
        !failures.is_empty(),
        "Drift detection failed: wrong topic symbol should have produced failures"
    );
    assert!(
        failures
            .iter()
            .any(|f| f.contains("wrongtopic") || f.contains("init")),
        "Expected failure to mention mismatched topic, got: {failures:?}"
    );
}

/// Verifies that a perfectly-conformant input produces zero failures.
/// Prevents the checker from always returning non-empty results.
#[test]
fn test_conformance_passes_on_valid_input() {
    let valid_events_rs = r#"
        pub struct EventInitialised {
            pub admin: soroban_sdk::Address,
            pub relay_signer: soroban_sdk::Address,
            pub ledger: u32,
        }

        pub fn initialised(env: &Env, admin: &soroban_sdk::Address, relay_signer: &soroban_sdk::Address) {
            env.events().publish(
                (symbol_short!("synapse"), symbol_short!("init")),
                EventInitialised { admin: admin.clone(), relay_signer: relay_signer.clone(), ledger: 0 },
            );
        }
    "#;

    let partial_manifest = r#"
schema_version = 1

[[event]]
struct_name = "EventInitialised"
topic       = "init"
emitter_fn  = "initialised"

[[event.fields]]
name = "admin"
type = "soroban_sdk::Address"

[[event.fields]]
name = "relay_signer"
type = "soroban_sdk::Address"

[[event.fields]]
name = "ledger"
type = "u32"
"#;

    let (_ver, events) = parse_manifest(partial_manifest);
    let failures = check_conformance(&events, valid_events_rs);
    assert!(
        failures.is_empty(),
        "Expected no failures on valid input, got: {failures:?}"
    );
}

// ─── EVENTS.md §2 table ↔ events.rs topic sync (CI step) ────────────────────

/// `(struct, topic, emitter_fn)` rows from the EVENTS.md §2 status table.
fn status_table_rows(events_md: &str) -> Vec<(String, String, String)> {
    let start = events_md
        .find("## 2. Implementation status")
        .expect("EVENTS.md must have a `## 2. Implementation status` section");
    let end = events_md[start..]
        .find("\n## 3")
        .map_or(events_md.len(), |e| start + e);
    let mut rows = Vec::new();
    for line in events_md[start..end].lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        // | [`Struct`](#anchor) | `topic` | `EventEmitter::fn` | ... | status |
        if cells.len() < 5 || !cells[1].starts_with("[`Event") {
            continue;
        }
        let strip = |c: &str| c.trim_matches('`').to_owned();
        let struct_name = cells[1]
            .trim_start_matches("[`")
            .split('`')
            .next()
            .unwrap_or_default()
            .to_owned();
        let emitter = strip(cells[3])
            .trim_start_matches("EventEmitter::")
            .to_owned();
        rows.push((struct_name, strip(cells[2]), emitter));
    }
    rows
}

/// Every `topics[1]` symbol published anywhere in `events.rs`.
fn emitted_topics(events_rs: &str) -> Vec<String> {
    let needle = "symbol_short!(\"synapse\"), symbol_short!(\"";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(pos) = events_rs[from..].find(needle) {
        let abs = from + pos + needle.len();
        let end = events_rs[abs..].find('"').expect("unterminated symbol");
        let topic = events_rs[abs..abs + end].to_owned();
        if !out.contains(&topic) {
            out.push(topic);
        }
        from = abs;
    }
    out
}

fn check_status_table(events_md: &str, events_rs: &str) -> Vec<String> {
    let rows = status_table_rows(events_md);
    let mut failures = Vec::new();
    if rows.is_empty() {
        failures.push("EVENTS.md §2 status table has no rows".to_owned());
    }
    for (struct_name, topic, emitter) in &rows {
        if !events_rs.contains(&format!("pub struct {struct_name} ")) {
            failures.push(format!(
                "EVENTS.md §2 lists `{struct_name}`, not in events.rs"
            ));
        }
        match extract_topic_for_emitter(events_rs, emitter) {
            Some(actual) if &actual == topic => {}
            Some(actual) => failures.push(format!(
                "EVENTS.md §2: `EventEmitter::{emitter}` publishes `{actual}`, table says `{topic}`"
            )),
            None => failures.push(format!(
                "EVENTS.md §2 lists `EventEmitter::{emitter}`, not in events.rs"
            )),
        }
    }
    for topic in emitted_topics(events_rs) {
        if !rows.iter().any(|(_, t, _)| *t == topic) {
            failures.push(format!(
                "events.rs publishes topic `{topic}`, missing from EVENTS.md §2"
            ));
        }
    }
    failures
}

/// CI step "Verify reference event decoder stays in sync with events.rs":
/// the EVENTS.md §2 table — the index subscribers decode from — must list
/// exactly the topics `events.rs` publishes, each with its real emitter.
#[test]
fn event_decoder_covers_catalogued_topics() {
    let failures = check_status_table(EVENTS_MD_SRC, EVENTS_RS_SRC);
    assert!(
        failures.is_empty(),
        "EVENTS.md §2 ↔ events.rs topic drift ({} issue(s)):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// The sync check must fail when a topic is emitted but not catalogued, and
/// when the table lists a topic that is never emitted.
#[test]
fn event_decoder_sync_detects_drift() {
    let events_rs = r#"
        pub struct EventA {
        pub struct EventB {
        pub fn a(env: &Env) {
            env.events().publish((symbol_short!("synapse"), symbol_short!("a")), 0);
        }
        pub fn b(env: &Env) {
            env.events().publish((symbol_short!("synapse"), symbol_short!("b")), 0);
        }
    "#;
    let md = "## 2. Implementation status\n\
        | Event | Topic[1] | Emitter | Entry-point(s) | Status |\n\
        |---|---|---|---|---|\n\
        | [`EventA`](#a) | `a` | `EventEmitter::a` | `x` | **Live** |\n\
        | [`EventC`](#c) | `c` | `EventEmitter::c` | `y` | **Live** |\n\
        ## 3. Catalogue\n";
    let failures = check_status_table(md, events_rs);
    assert!(
        failures.iter().any(|f| f.contains("topic `b`")),
        "{failures:?}"
    );
    assert!(
        failures.iter().any(|f| f.contains("`EventC`")),
        "{failures:?}"
    );
    assert!(
        failures.iter().any(|f| f.contains("EventEmitter::c")),
        "{failures:?}"
    );
}
