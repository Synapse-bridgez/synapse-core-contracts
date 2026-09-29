# ADR-0005 — Bounded `upgrade_and_migrate` + migration registry

> **Status:** Accepted
> **Date:** 2026-Q3
> **Deciders:** Synapse Bridge core team, contract author
> **Issues:** #82 (mechanism); coordinates with #83 rollback guard

---

## Context

Same-schema in-place upgrades leave storage untouched. Schema-changing
work (compact encoding, key namespacing) needs an on-chain migration
primitive with clear failure semantics and resource bounds.

---

## Options considered

### Option A — Atomic migrate-then-swap with versioned registry ✅

`upgrade_and_migrate(new_wasm_hash, expected_schema_version, migration_id)`:

1. Schema-range check
2. Dispatch `migration_id` in `migration.rs` (routines shipped in **this** WASM)
3. On success, shared WASM-swap primitive with `LastUpgradeMigrated = true`
4. On any `Err`, Soroban aborts the invoke — storage byte-identical, no swap

**Bound:** `MAX_MIGRATION_STORAGE_TOUCHES = 64`. Planned work above the cap
returns `MigrationBoundExceeded` before further touches.

**Resumability (v1 out of scope):** Migrations that cannot fit in one call
need a multi-call state machine (cursor + checkpoint keys). Document the
limitation; do not ship a half-built resumable runner in v1.

### Option B — Post-upgrade `migrate()` in a second transaction

Not atomic from the operator's point of view; rejected for the Wave's
safety bar.

---

## Decision

Adopt Option A. Interaction with rollback (#83): if `LastUpgradeMigrated`,
`rollback_upgrade` returns `UpgradeNotReversible`.

---

## Consequences

- **Positive:** Reusable, auditable migration scaffolding for sibling issues.
- **Negative:** Migration code for schema N→N+1 must be present in the WASM
  that performs the call (typically a bridge build that understands both
  layouts), then the new WASM is installed.
- **Follow-up:** Resumable multi-call migrations when a real oversized
  migration is designed.

## References

- `src/migration.rs`, `src/lib.rs` — `upgrade_and_migrate`
- `src/test_upgrade_safety.rs` — atomic revert test
