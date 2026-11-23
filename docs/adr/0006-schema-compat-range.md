# ADR-0006 — Schema-version compatibility ranges

> **Status:** Accepted
> **Date:** 2026-Q3
> **Deciders:** Synapse Bridge core team, contract author
> **Issues:** #84
> **Amends:** [ADR-0003](./0003-upgrade-schema-version-guard.md)

---

## Context

ADR-0003's exact-match `expected_schema_version` guard is maximally safe
and maximally rigid. Additive, backward-compatible schema changes force
every upgrade call to be re-coordinated with an exact version number.

---

## Options considered

### Option A — Admin-declared `[min, max]` window ✅

- `set_schema_compatibility_range(min, max)` — admin-gated
- Must include the **current** on-chain schema version or reject with
  `InvalidSchemaCompatRange` (never create an un-upgradeable contract)
- `upgrade` / `propose_upgrade` / migrate / rollback path check
  `expected ∈ [min, max]`
- **Default when unset:** `min = max = schema_version()` — identical to
  ADR-0003 exact-match behaviour for existing deployments

### Option B — Infer compatibility from schema diffing

Out of scope (no static analysis on-chain).

---

## Decision

Adopt Option A as an explicit, opt-in weakening of ADR-0003 with a
default-safe fallback.

---

## Consequences

- **Positive:** Additive upgrades can proceed within an admin-declared window
  without weakening protection against out-of-range mismatches.
- **Negative:** A too-wide range is an operational foot-gun — mitigated by
  multisig admin requirements and monitoring of the setter (no dedicated
  event in v1; can be added later).

## `compare_schema_versions` — accepted pairs and `SchemaDiff` output

`compare_schema_versions(env, manifest, from_version, to_version)` in
`src/migration.rs` is a **read-only, informational helper** for operator
planning. It does not enforce the compatibility range at upgrade time; that
enforcement is done by the upgrade entry points against the stored
`[min, max]` window. The function documents what has *changed* between any
two schema versions.

### Version-pair handling

| Condition | Behaviour |
|---|---|
| `from_version == to_version` | Returns an empty `SchemaDiff` (`empty == true`, `changes` is empty). No changes are introduced when the versions are identical. |
| `from_version < to_version` | Returns all manifest entries with `version > from_version && version <= to_version`, in ascending order. |
| `from_version > to_version` | Arguments are **silently swapped** so that `lo = to_version` and `hi = from_version`. The result covers the same range as the forward direction. This allows callers to pass the pair in either order without error. |

There is **no invalid pair**: every `(from_version, to_version)` combination
is accepted. Pairs that fall entirely outside the manifest's recorded versions
produce an empty `SchemaDiff`.

### `SchemaDiff` fields

| Field | Type | Meaning |
|---|---|---|
| `from_version` | `u32` | The lower of the two versions (`lo` after any swap). |
| `to_version` | `u32` | The higher of the two versions (`hi` after any swap). |
| `changes` | `Vec<SchemaChange>` | Manifest entries in the half-open range `(from_version, to_version]`, in manifest order (ascending version). Empty when no entries fall in range. |
| `empty` | `bool` | `true` when `changes` is empty. Callers should render "no additive changes recorded" rather than treating this as an error. |

### Caller guidance

- An `empty == true` result means no manifest entries were recorded between
  the two versions. This is normal for version pairs that pre-date the
  manifest or for ranges that only contain non-additive changes not captured
  as `SchemaChange` entries.
- The function never fails or returns an error. All error handling is the
  caller's responsibility (e.g. rendering the diff for an operator).
- For upgrade gate enforcement, see `set_schema_compatibility_range` and
  `schema_compatibility_range` in `src/lib.rs`.

## References

- `src/migration.rs` — `compare_schema_versions`, `SchemaDiff`, `SchemaChange`, `MigrationManifest`
- `src/lib.rs` — `set_schema_compatibility_range`, `schema_compatibility_range`
- ADR-0003
