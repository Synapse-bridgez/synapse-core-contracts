# ADR-0003 — Upgrade schema-version guard

> **Status:** Accepted
> **Date:** 2026-Q3
> **Deciders:** Synapse Bridge core team, contract author

---

## Context

[`DECISIONS.md`](../../DECISIONS.md) already established that
`synapse-core-contract` uses in-place upgrades, and named the central risk
explicitly: "the new WASM must be compatible with the existing storage
schema... A schema-breaking upgrade is a bug, not a feature." THREAT_MODEL.md's
self-review (finding F-04) noted that, beyond that documentation, there was
no on-chain mechanism at all — `upgrade(new_wasm_hash)` would deploy any
WASM the admin authorised, with nothing checking that the caller's
assumptions about the current on-chain state matched reality.

A fundamental platform constraint shapes what's achievable here: **Soroban
gives the currently-running contract no way to introspect an
uploaded-but-not-yet-installed WASM blob.** `env.deployer().update_current_contract_wasm()`
takes only a hash; there is no API to ask "does this WASM's
`Transaction`/`StorageKey` layout match what I have in storage right now?"
before installing it. Any on-chain guard can therefore only validate
something about *current* state at the moment `upgrade()` is called — never
the incoming binary's actual compatibility.

---

## Options considered

### Option A — On-chain schema-version assertion, checked against current state ✅ (chosen)

Store a `SCHEMA_VERSION` constant (set at `initialize()`) in
`StorageKey::SchemaVersion`. `upgrade()` takes a new
`expected_schema_version` argument and rejects the call with
`SchemaVersionMismatch` if it doesn't match on-chain state, before touching
WASM.

**Pros:**
- Catches a real, if narrow, class of operator error: invoking `upgrade()`
  against a contract instance whose on-chain state isn't what the caller
  believes (wrong contract ID passed to tooling, a concurrent upgrade
  already landed, stale deployment scripts) is now a `SchemaVersionMismatch`
  error instead of a silent bad upgrade.
- Cheap: one persistent `u32` read, one comparison, no new WASM-introspection
  capability required (which doesn't exist on Soroban regardless).
- `schema_version()` is a public query, so `DEPLOYMENT.md`'s upgrade
  runbook can read the expected value back before constructing the upgrade
  transaction, rather than the operator having to track it out-of-band.
- `EventContractUpgraded` gaining the `schema_version` field (an additive,
  Minor-per-EVENTS.md-policy change) gives auditors a permanent on-chain
  record of which schema state each upgrade was checked against.

**Cons:**
- Does **not** solve the problem F-04's title names — it cannot validate
  that the *new* WASM is actually schema-compatible, only that the caller's
  belief about the *current* on-chain schema matches reality. The
  fundamentally harder problem (verifying an unbuilt/uninstalled binary's
  compatibility) is out of scope for what any on-chain check can achieve.
- Requires bumping `SCHEMA_VERSION` and threading the new value through
  deployment tooling on every future schema-changing upgrade — a manual
  discipline the contract cannot enforce on its own.

### Option B — No guard; rely entirely on `DECISIONS.md`'s documented requirement

Leave `upgrade()` as-is and treat "the new WASM must be schema-compatible"
as a purely operational/documentation requirement.

**Pros:**
- No implementation cost; no new argument on a security-sensitive entry
  point for admin tooling to get right.

**Cons:**
- This is the status quo F-04 was raised against: nothing on-chain
  distinguishes a deliberate, verified upgrade from an operator error, and
  the failure mode (silent persistent-storage corruption) is severe enough
  that THREAT_MODEL.md rated the finding Medium even with the documentation
  in place.

### Option C — Full schema self-description (new WASM asserts compatibility post-upgrade)

Have the *new* WASM run a `migrate()`-style entry point immediately after
installation, asserting its own compatibility with what it finds in storage
and updating `SchemaVersion` itself.

**Pros:**
- Closer to actually validating the new binary, since it runs *as* the new
  binary rather than checking beforehand.

**Cons:**
- Meaningfully more complexity (a required post-upgrade call, migration
  state tracking, handling a WASM that forgets to call it) for a contract
  that has had exactly one schema version to date and no migrations to
  exercise this against. Worth reconsidering if and when a real
  schema-changing upgrade is actually needed — premature to build now.

---

## Decision

**Adopt Option A.** The guard is honestly scoped as a caller-state
assertion, not a new-binary compatibility proof — the doc comments on
`upgrade()`, this ADR, and THREAT_MODEL.md's F-04 entry all say so
explicitly, rather than overclaiming a stronger guarantee than Soroban lets
any on-chain check actually provide.

---

## Consequences

- **Positive:** A meaningful, cheap class of operator error is now caught
  before WASM is touched. `schema_version()` gives deployment tooling and
  auditors a way to read and record the value independent of trusting
  `contract-ids.json` alone (mirrors the rationale for the existing
  `admin()`/`relay_signer()` queries).
- **Negative / accepted trade-offs:** `SCHEMA_VERSION` is a manually
  maintained constant — nothing forces a future contributor to bump it when
  `Transaction`/`StorageKey` layout actually changes. This is a discipline
  requirement, documented here and in `types.rs`'s doc comment on
  `SCHEMA_VERSION`, not an enforced one.
- **Follow-up work:** Option C (self-asserting migration) is worth
  revisiting once a real schema-breaking upgrade is actually planned, when
  there is a concrete migration to design against instead of a hypothetical
  one.

---

## References

- [`src/lib.rs`](../../src/lib.rs) — `upgrade()`, `schema_version()`
- [`src/types.rs`](../../src/types.rs) — `SCHEMA_VERSION` constant
- [`src/storage.rs`](../../src/storage.rs) — `SchemaVersion` storage helpers
- [`src/test_pause.rs`](../../src/test_pause.rs) — mismatch-rejection and
  query tests
- [`THREAT_MODEL.md`](../../THREAT_MODEL.md) — finding F-04
- [`DECISIONS.md`](../../DECISIONS.md) — the original in-place-upgradability
  decision this one extends
- [`EVENTS.md`](../../EVENTS.md) — `EventContractUpgraded`'s `schema_version` field
- [`DEPLOYMENT.md`](../../DEPLOYMENT.md) — upgrade runbook's `schema_version()` read-back step
