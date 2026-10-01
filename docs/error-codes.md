# Contract Error Codes

This document is the companion reference for the structured, machine-parseable
diagnostic surface exposed by `ContractError` in `src/types.rs`. It enumerates
every error code together with the intended off-chain handling guidance so that
the relay service in `synapse-core` can triage failed calls without
pattern-matching on human-readable strings.

## Structured diagnostic shape

Every `ContractError` variant carries a stable numeric code and a small set of
well-typed context fields (never free-text). The code is exposed via
`ContractError::code()` and the context via `ContractError::context()`, both of
which return values that are safe to serialize and parse off-chain.

| Field | Type | Meaning |
| --- | --- | --- |
| `code` | `u32` | Stable numeric identifier for the variant. Never reused or renumbered. |
| `context` | `ErrorContext` | Small, non-sensitive, well-typed detail describing the failure. |

`ErrorContext` is a fixed-shape struct so that consumers can decode it without
variant-specific branching:

| Field | Type | Meaning |
| --- | --- | --- |
| `category` | `ErrorCategory` | Coarse classification used for routing. |
| `retry_safe` | `bool` | Whether the same call may be retried unchanged. |
| `alert_worthy` | `bool` | Whether the failure should page an operator. |
| `subject` | `u32` | Identifier of the affected entity (e.g. dispute id, proposal id); `0` when not applicable. |

## Handling guidance

`retry_safe` and `alert_worthy` are the two flags the relay service should key
its triage on:

- **retry-safe** — the call may be re-submitted unchanged (transient or
  ordering-related failure).
- **alert-worthy** — the failure indicates a condition an operator must
  investigate (invariant violation, authorization failure, or corruption).

Routine, non-retry-safe, non-alert-worthy failures should be dropped.

## Error code reference

| Code | Variant | Category | Retry-safe | Alert-worthy | Notes |
| --- | --- | --- | --- | --- | --- |
| 1 | `Unauthorized` | `Auth` | no | yes | Caller failed an authorization check. |
| 2 | `NotFound` | `Lookup` | no | no | Referenced entity does not exist. |
| 3 | `InvalidInput` | `Validation` | no | no | Arguments failed validation. |
| 4 | `AlreadyExists` | `Validation` | no | no | Entity already present; drop the call. |
| 5 | `InsufficientFunds` | `Funds` | no | yes | Balance too low to satisfy the operation. |
| 6 | `DisputeClosed` | `State` | no | no | Operation attempted against a closed dispute. |
| 7 | `StateConflict` | `State` | yes | no | Transient state mismatch; safe to retry. |
| 8 | `Internal` | `Internal` | no | yes | Invariant violation; requires investigation. |

> The table above is kept in lockstep with the `ContractError` enum by the
> completeness test in `src/lib.rs`, which enumerates every variant and asserts
> that each exposes a code, a category, and the two triage flags. Any new
> variant added to the enum must be reflected here and in that test.

## Coordination

This document describes the contract-side structured-error surface only. The
actual off-chain error-handling and alerting logic that consumes it lives in
`synapse-core` and is owned by the relay service team; wiring the relay's
triage logic to `code()` / `context()` is a direct coordination item for that
team.
