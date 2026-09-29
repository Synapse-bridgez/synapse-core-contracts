# ADR-0004 — Timelocked upgrade propose / finalize / cancel

> **Status:** Accepted
> **Date:** 2026-Q3
> **Deciders:** Synapse Bridge core team, contract author
> **Issues:** #81, interaction with #83

---

## Context

THREAT_MODEL.md §8 **R-05** accepted "in-place upgrade — no timelock" as a
Phase 1 residual risk. Immediate `upgrade()` gives guardians, subscriber
teams, and monitoring no on-chain window to review a pending WASM change
before it takes effect.

---

## Options considered

### Option A — Propose / finalize / cancel with configurable ledger delay ✅

`propose_upgrade` records `(wasm_hash, expected_schema_version, eta_ledger)`,
`finalize_upgrade` runs the existing WASM-swap primitive only after ETA,
`cancel_upgrade` clears the pending slot. `get_pending_upgrade` exposes the
tuple for off-chain tooling. Distinct events (`up_prop` / `up_fin` / `up_can`)
sit alongside the existing `upgrade` event.

**Second propose while pending:** **replace** (overwrite + restart delay),
matching `propose_admin` semantics.

**Immediate `upgrade()`:** retained for emergency use, rollback, and as the
shared swap primitive — production ops SHOULD prefer the timelocked path.

### Option B — Replace `upgrade()` entirely with timelock-only

Removes the emergency fast path; rejected for incident-response flexibility.

---

## Decision

Adopt Option A. Default delay = `DEFAULT_UPGRADE_DELAY_LEDGERS` (17_280 ≈ 24h),
overridable via `set_upgrade_delay`.

---

## Consequences

- **Positive:** R-05 moves from accepted to mitigated (see THREAT_MODEL.md).
- **Negative:** Operators must coordinate propose → wait → finalize; a
  second propose restarts the clock (documented, tested).
- **Follow-up:** Optional forced-pause-before-finalize remains F-05.

## References

- `src/lib.rs` — `propose_upgrade`, `finalize_upgrade`, `cancel_upgrade`
- `src/test_upgrade_safety.rs`
- THREAT_MODEL.md §8 R-05
