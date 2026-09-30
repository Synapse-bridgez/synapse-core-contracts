//! CI helper: verify the built release WASM's `schema_version()` matches
//! `migrations.toml` exactly (#88).
//!
//! Enabled only when `SYNAPSE_SCHEMA_CHECK_WASM` is set (the CI / make target
//! sets it after a single release WASM build). Normal `cargo test` still runs
//! the constant↔manifest exact-match guard.

#![cfg(test)]

extern crate std;

use soroban_sdk::{testutils::Address as _, Address, Bytes, BytesN, Env};

use crate::types::SCHEMA_VERSION;
use crate::SynapseCoreContractClient;

/// Parse `schema_version = N` entries from a migrations.toml body.
fn manifest_versions(body: &str) -> std::vec::Vec<u32> {
    let mut out = std::vec::Vec::new();
    for line in body.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("schema_version") {
            let rest = rest.trim().trim_start_matches('=').trim();
            if let Ok(v) = rest.parse::<u32>() {
                out.push(v);
            }
        }
    }
    out
}

#[test]
fn schema_version_constant_has_exact_manifest_entry() {
    let body = include_str!("../migrations.toml");
    let versions = manifest_versions(body);
    assert!(
        versions.contains(&SCHEMA_VERSION),
        "SCHEMA_VERSION={SCHEMA_VERSION} has no exact matching entry in migrations.toml\n\
         Found versions: {versions:?}\n\
         Add a [[migrations]] block with schema_version = {SCHEMA_VERSION} and a note."
    );
    assert!(
        !versions.is_empty(),
        "migrations.toml must list at least one schema_version"
    );
}

#[test]
fn schema_version_from_release_wasm_matches_manifest() {
    let wasm_path = match std::env::var("SYNAPSE_SCHEMA_CHECK_WASM") {
        Ok(p) => p,
        Err(_) => {
            // Not in the CI schema-check mode; the constant↔manifest test above
            // still guards drift for everyday `cargo test`.
            return;
        }
    };
    let manifest_path =
        std::env::var("SYNAPSE_SCHEMA_CHECK_MANIFEST").unwrap_or_else(|_| "migrations.toml".into());

    let wasm_bytes = std::fs::read(&wasm_path).unwrap_or_else(|e| {
        panic!("failed to read release WASM at {wasm_path}: {e}");
    });
    let manifest = std::fs::read_to_string(&manifest_path).unwrap_or_else(|e| {
        panic!("failed to read migrations manifest at {manifest_path}: {e}");
    });
    let versions = manifest_versions(&manifest);

    let env = Env::default();
    let wasm = Bytes::from_slice(&env, &wasm_bytes);
    let wasm_hash: BytesN<32> = env.deployer().upload_contract_wasm(wasm);
    // Deploy the *release* WASM into an ephemeral Soroban Env and invoke it.
    let contract_id = env.register(wasm_bytes.as_slice(), ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    client.register_installed_wasm(&wasm_hash);

    let reported = client.schema_version();
    assert!(
        versions.contains(&reported),
        "\nCI schema check failed (#88).\n\
         schema_version() from release WASM returned {reported}, but {manifest_path}\n\
         has no exact matching [[migrations]] entry.\n\
         Found versions in manifest: {versions:?}\n\n\
         Fix: add:\n\
           [[migrations]]\n\
           schema_version = {reported}\n\
           note = \"<human-readable migration guidance>\"\n\
         to {manifest_path} (and update DECISIONS.md / CHANGELOG.md).\n"
    );
    assert_eq!(
        reported, SCHEMA_VERSION,
        "release WASM schema_version()={reported} != SCHEMA_VERSION={SCHEMA_VERSION}"
    );
}
