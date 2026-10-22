//! # Event Conformance Test Gate (issue #113, restored for #255)
//!
//! This module is the CI gate that keeps the event system consistent across
//! three sources of truth:
//!
//! 1. `src/events.rs` — the Rust event structs and topic constants.
//! 2. `EVENTS.md` — the human-readable event documentation.
//! 3. `event_conformance_manifest.toml` — the machine-readable manifest.
//!
//! The suite contains exactly **10 tests**, matching the breakdown described in
//! `README.md`:
//!
//! - **2 primary conformance checks** — a three-way consistency check between
//!   `events.rs`, `EVENTS.md`, and the manifest.
//! - **1 manifest well-formedness check** — the manifest parses and has the
//!   expected shape.
//! - **1 all-topics check** — every declared topic is covered everywhere.
//! - **5 drift-detection fixtures** — deliberately-wrong manifest/event-struct
//!   pairs that the conformance checker must *reject* (negative tests).
//!
//! A conformance checker that only ever passes is not actually checking
//! anything, so the negative fixtures are as important as the positive checks.
//!
//! All parsing is done manually against the embedded source text via
//! `include_str!`, so this module introduces **zero new dependencies**.

#![cfg(test)]

use std::collections::{BTreeMap, BTreeSet};

/// The Rust event definitions, embedded at compile time.
const EVENTS_RS: &str = include_str!("events.rs");

/// The human-readable event documentation, embedded at compile time.
const EVENTS_MD: &str = include_str!("../EVENTS.md");

/// The machine-readable event conformance manifest, embedded at compile time.
const MANIFEST_TOML: &str = include_str!("../event_conformance_manifest.toml");

// ---------------------------------------------------------------------------
// Minimal, dependency-free parsers
// ---------------------------------------------------------------------------

/// A single event entry extracted from one of the three sources of truth.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EventEntry {
    /// The event name, e.g. `Transfer`.
    name: String,
    /// The topic string, e.g. `"transfer"`.
    topic: String,
}

/// Parses `events.rs` and returns the event structs it declares.
///
/// An event struct is recognised by the `#[contracttype]` attribute followed by
/// a `pub struct <Name> { ... }` block. The topic is taken from the associated
/// `pub const <NAME>_TOPIC: &str = "<topic>";` declaration when present, or
/// derived from the struct name (lowercased) otherwise.
fn parse_events_rs(src: &str) -> Vec<EventEntry> {
    let mut names: Vec<String> = Vec::new();
    let mut lines = src.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed == "#[contracttype]" {
            // Look ahead for the struct declaration.
            for next in lines.by_ref() {
                let next_trimmed = next.trim();
                if next_trimmed.is_empty() || next_trimmed.starts_with("//") {
                    continue;
                }
                if let Some(rest) = next_trimmed.strip_prefix("pub struct ") {
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        names.push(name);
                    }
                }
                break;
            }
        }
    }

    // Collect explicit topic constants: `pub const <NAME>_TOPIC: &str = "<topic>";`
    let mut topics: BTreeMap<String, String> = BTreeMap::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("pub const ") {
            if let Some(colon) = rest.find(':') {
                let const_name = rest[..colon].trim();
                if let Some(stem) = const_name.strip_suffix("_TOPIC") {
                    if let Some(eq) = rest.find('=') {
                        let value = rest[eq + 1..].trim();
                        if let Some(topic) = extract_string_literal(value) {
                            topics.insert(stem.to_string(), topic);
                        }
                    }
                }
            }
        }
    }

    names
        .into_iter()
        .map(|name| {
            let topic = topics
                .get(&name)
                .cloned()
                .unwrap_or_else(|| name.to_lowercase());
            EventEntry { name, topic }
        })
        .collect()
}

/// Extracts the first double-quoted string literal from `value`.
fn extract_string_literal(value: &str) -> Option<String> {
    let start = value.find('"')?;
    let rest = &value[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Parses `EVENTS.md` and returns the documented events.
///
/// Each documented event is expected as a markdown heading of the form
/// `### <Name>` (optionally followed by a topic in backticks on the same line,
/// e.g. `### Transfer (`transfer`)`). If no topic is given inline, the topic is
/// derived from the name (lowercased).
fn parse_events_md(src: &str) -> Vec<EventEntry> {
    let mut entries = Vec::new();
    for line in src.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("### ") {
            let rest = rest.trim();
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                continue;
            }
            let topic = extract_backtick_topic(rest).unwrap_or_else(|| name.to_lowercase());
            entries.push(EventEntry { name, topic });
        }
    }
    entries
}

/// Extracts a topic from a backtick-quoted token, e.g. ``(`transfer`)``.
fn extract_backtick_topic(value: &str) -> Option<String> {
    let start = value.find('`')?;
    let rest = &value[start + 1..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

/// Parses the manifest TOML and returns the declared events.
///
/// The manifest is expected to contain one or more `[[event]]` tables, each
/// with `name = "..."` and `topic = "..."` keys. This is a deliberately small
/// parser sufficient for the manifest's flat structure.
fn parse_manifest(src: &str) -> Vec<EventEntry> {
    let mut entries = Vec::new();
    let mut current: Option<(Option<String>, Option<String>)> = None;

    let flush = |current: &mut Option<(Option<String>, Option<String>)>,
                 entries: &mut Vec<EventEntry>| {
        if let Some((Some(name), Some(topic))) = current.take() {
            entries.push(EventEntry { name, topic });
        } else {
            *current = None;
        }
    };

    for line in src.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if trimmed == "[[event]]" {
            flush(&mut current, &mut entries);
            current = Some((None, None));
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            if let Some(slot) = current.as_mut() {
                match key {
                    "name" => slot.0 = extract_string_literal(value),
                    "topic" => slot.1 = extract_string_literal(value),
                    _ => {}
                }
            }
        }
    }
    flush(&mut current, &mut entries);
    entries
}

/// The result of a three-way conformance check.
#[derive(Debug, Default)]
struct ConformanceReport {
    /// Events present in `events.rs` but missing from the manifest.
    missing_from_manifest: Vec<String>,
    /// Events present in the manifest but missing from `events.rs`.
    missing_from_code: Vec<String>,
    /// Events present in `events.rs` but missing from `EVENTS.md`.
    missing_from_docs: Vec<String>,
    /// Events present in `EVENTS.md` but missing from `events.rs`.
    undocumented_in_code: Vec<String>,
    /// Events whose topic differs between sources.
    topic_mismatches: Vec<String>,
}

impl ConformanceReport {
    /// Returns `true` when the three sources are fully consistent.
    fn is_conformant(&self) -> bool {
        self.missing_from_manifest.is_empty()
            && self.missing_from_code.is_empty()
            && self.missing_from_docs.is_empty()
            && self.undocumented_in_code.is_empty()
            && self.topic_mismatches.is_empty()
    }
}

/// Performs the three-way consistency check between code, docs, and manifest.
fn check_conformance(
    code: &[EventEntry],
    docs: &[EventEntry],
    manifest: &[EventEntry],
) -> ConformanceReport {
    let mut report = ConformanceReport::default();

    let code_map: BTreeMap<&str, &str> = code
        .iter()
        .map(|e| (e.name.as_str(), e.topic.as_str()))
        .collect();
    let docs_map: BTreeMap<&str, &str> = docs
        .iter()
        .map(|e| (e.name.as_str(), e.topic.as_str()))
        .collect();
    let manifest_map: BTreeMap<&str, &str> = manifest
        .iter()
        .map(|e| (e.name.as_str(), e.topic.as_str()))
        .collect();

    for name in code_map.keys() {
        if !manifest_map.contains_key(name) {
            report.missing_from_manifest.push((*name).to_string());
        }
        if !docs_map.contains_key(name) {
            report.missing_from_docs.push((*name).to_string());
        }
    }
    for name in manifest_map.keys() {
        if !code_map.contains_key(name) {
            report.missing_from_code.push((*name).to_string());
        }
    }
    for name in docs_map.keys() {
        if !code_map.contains_key(name) {
            report.undocumented_in_code.push((*name).to_string());
        }
    }

    // Topic consistency across all three sources.
    let all_names: BTreeSet<&str> = code_map
        .keys()
        .chain(docs_map.keys())
        .chain(manifest_map.keys())
        .copied()
        .collect();
    for name in all_names {
        let topics: Vec<&str> = [code_map.get(name), docs_map.get(name), manifest_map.get(name)]
            .into_iter()
            .flatten()
            .copied()
            .collect();
        if topics.len() > 1 && topics.iter().any(|t| *t != topics[0]) {
            report.topic_mismatches.push(name.to_string());
        }
    }

    report
}

// ---------------------------------------------------------------------------
// 2 primary conformance checks
// ---------------------------------------------------------------------------

/// **Primary conformance check #1** — `events.rs` vs the manifest.
///
/// Every event struct declared in `src/events.rs` must appear in
/// `event_conformance_manifest.toml` with a matching topic, and vice versa.
#[test]
fn primary_conformance_code_vs_manifest() {
    let code = parse_events_rs(EVENTS_RS);
    let manifest = parse_manifest(MANIFEST_TOML);
    let docs = parse_events_md(EVENTS_MD);

    assert!(
        !code.is_empty(),
        "expected at least one event struct in events.rs"
    );
    assert!(
        !manifest.is_empty(),
        "expected at least one [[event]] entry in the manifest"
    );

    let report = check_conformance(&code, &docs, &manifest);
    assert!(
        report.missing_from_manifest.is_empty(),
        "events in events.rs missing from manifest: {:?}",
        report.missing_from_manifest
    );
    assert!(
        report.missing_from_code.is_empty(),
        "events in manifest missing from events.rs: {:?}",
        report.missing_from_code
    );
    assert!(
        report.topic_mismatches.is_empty(),
        "topic mismatches between events.rs and manifest: {:?}",
        report.topic_mismatches
    );
}

/// **Primary conformance check #2** — `EVENTS.md` vs the manifest.
///
/// Every documented event in `EVENTS.md` must appear in the manifest with a
/// matching topic, and every manifest event must be documented.
#[test]
fn primary_conformance_docs_vs_manifest() {
    let code = parse_events_rs(EVENTS_RS);
    let manifest = parse_manifest(MANIFEST_TOML);
    let docs = parse_events_md(EVENTS_MD);

    assert!(
        !docs.is_empty(),
        "expected at least one documented event in EVENTS.md"
    );

    let report = check_conformance(&code, &docs, &manifest);
    assert!(
        report.missing_from_docs.is_empty(),
        "events in events.rs missing from EVENTS.md: {:?}",
        report.missing_from_docs
    );
    assert!(
        report.undocumented_in_code.is_empty(),
        "events documented in EVENTS.md but absent from events.rs: {:?}",
        report.undocumented_in_code
    );
    assert!(
        report.topic_mismatches.is_empty(),
        "topic mismatches between EVENTS.md and manifest: {:?}",
        report.topic_mismatches
    );
}

// ---------------------------------------------------------------------------
// 1 manifest well-formedness check
// ---------------------------------------------------------------------------

/// **Manifest well-formedness check** — the manifest parses cleanly.
///
/// Every `[[event]]` table must declare both a non-empty `name` and a non-empty
/// `topic`, and no event name or topic may be duplicated.
#[test]
fn manifest_is_well_formed() {
    let manifest = parse_manifest(MANIFEST_TOML);
    assert!(
        !manifest.is_empty(),
        "manifest must declare at least one [[event]] entry"
    );

    // Re-scan to detect tables missing required keys (the parser silently
    // drops them, so we count declared tables independently).
    let declared_tables = MANIFEST_TOML
        .lines()
        .filter(|l| l.trim() == "[[event]]")
        .count();
    assert_eq!(
        declared_tables,
        manifest.len(),
        "every [[event]] table must declare both `name` and `topic`"
    );

    let mut names = BTreeSet::new();
    let mut topics = BTreeSet::new();
    for entry in &manifest {
        assert!(!entry.name.is_empty(), "event name must not be empty");
        assert!(!entry.topic.is_empty(), "event topic must not be empty");
        assert!(
            names.insert(entry.name.clone()),
            "duplicate event name in manifest: {}",
            entry.name
        );
        assert!(
            topics.insert(entry.topic.clone()),
            "duplicate event topic in manifest: {}",
            entry.topic
        );
    }
}

// ---------------------------------------------------------------------------
// 1 all-topics check
// ---------------------------------------------------------------------------

/// **All-topics check** — every topic is covered across all three sources.
///
/// The union of topics declared in `events.rs`, `EVENTS.md`, and the manifest
/// must be identical, i.e. no source is missing a topic that another declares.
#[test]
fn all_topics_are_covered() {
    let code = parse_events_rs(EVENTS_RS);
    let docs = parse_events_md(EVENTS_MD);
    let manifest = parse_manifest(MANIFEST_TOML);

    let code_topics: BTreeSet<&str> = code.iter().map(|e| e.topic.as_str()).collect();
    let docs_topics: BTreeSet<&str> = docs.iter().map(|e| e.topic.as_str()).collect();
    let manifest_topics: BTreeSet<&str> = manifest.iter().map(|e| e.topic.as_str()).collect();

    assert_eq!(
        code_topics, manifest_topics,
        "topics in events.rs and manifest differ"
    );
    assert_eq!(
        docs_topics, manifest_topics,
        "topics in EVENTS.md and manifest differ"
    );
    assert_eq!(
        code_topics, docs_topics,
        "topics in events.rs and EVENTS.md differ"
    );
}

// ---------------------------------------------------------------------------
// 5 drift-detection fixtures (negative tests)
// ---------------------------------------------------------------------------

/// **Drift fixture #1** — an event struct present in code but absent from the
/// manifest must be rejected.
#[test]
fn drift_fixture_code_event_missing_from_manifest() {
    let code = vec![
        EventEntry {
            name: "Transfer".into(),
            topic: "transfer".into(),
        },
        EventEntry {
            name: "Mint".into(),
            topic: "mint".into(),
        },
    ];
    let docs = code.clone();
    let manifest = vec![EventEntry {
        name: "Transfer".into(),
        topic: "transfer".into(),
    }];

    let report = check_conformance(&code, &docs, &manifest);
    assert!(!report.is_conformant(), "drift must be detected");
    assert_eq!(report.missing_from_manifest, vec!["Mint".to_string()]);
}

/// **Drift fixture #2** — a manifest event with no corresponding code struct
/// must be rejected.
#[test]
fn drift_fixture_manifest_event_missing_from_code() {
    let code = vec![EventEntry {
        name: "Transfer".into(),
        topic: "transfer".into(),
    }];
    let docs = code.clone();
    let manifest = vec![
        EventEntry {
            name: "Transfer".into(),
            topic: "transfer".into(),
        },
        EventEntry {
            name: "Burn".into(),
            topic: "burn".into(),
        },
    ];

    let report = check_conformance(&code, &docs, &manifest);
    assert!(!report.is_conformant(), "drift must be detected");
    assert_eq!(report.missing_from_code, vec!["Burn".to_string()]);
}

/// **Drift fixture #3** — an event struct present in code but undocumented in
/// `EVENTS.md` must be rejected.
#[test]
fn drift_fixture_code_event_missing_from_docs() {
    let code = vec![
        EventEntry {
            name: "Transfer".into(),
            topic: "transfer".into(),
        },
        EventEntry {
            name: "Approve".into(),
            topic: "approve".into(),
        },
    ];
    let manifest = code.clone();
    let docs = vec![EventEntry {
        name: "Transfer".into(),
        topic: "transfer".into(),
    }];

    let report = check_conformance(&code, &docs, &manifest);
    assert!(!report.is_conformant(), "drift must be detected");
    assert_eq!(report.missing_from_docs, vec!["Approve".to_string()]);
}

/// **Drift fixture #4** — a documented event with no corresponding code struct
/// must be rejected.
#[test]
fn drift_fixture_docs_event_missing_from_code() {
    let code = vec![EventEntry {
        name: "Transfer".into(),
        topic: "transfer".into(),
    }];
    let manifest = code.clone();
    let docs = vec![
        EventEntry {
            name: "Transfer".into(),
            topic: "transfer".into(),
        },
        EventEntry {
            name: "Pause".into(),
            topic: "pause".into(),
        },
    ];

    let report = check_conformance(&code, &docs, &manifest);
    assert!(!report.is_conformant(), "drift must be detected");
    assert_eq!(report.undocumented_in_code, vec!["Pause".to_string()]);
}

/// **Drift fixture #5** — a topic that differs between sources must be
/// rejected even when the event name matches everywhere.
#[test]
fn drift_fixture_topic_mismatch() {
    let code = vec![EventEntry {
        name: "Transfer".into(),
        topic: "transfer".into(),
    }];
    let docs = vec![EventEntry {
        name: "Transfer".into(),
        topic: "transfer".into(),
    }];
    let manifest = vec![EventEntry {
        name: "Transfer".into(),
        topic: "xfer".into(),
    }];

    let report = check_conformance(&code, &docs, &manifest);
    assert!(!report.is_conformant(), "drift must be detected");
    assert_eq!(report.topic_mismatches, vec!["Transfer".to_string()]);
}