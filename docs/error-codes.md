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

| Code | Variant | Category | Retry-safe | Alert-worthy | Notes | What happened | What to do |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `Unauthorized` | `Auth` | no | yes | Caller failed an authorization check. | The caller's signature or identity did not match the required admin or relay signer. | Verify the signing key being used matches the account that holds the required role. |
| 2 | `NotFound` | `Lookup` | no | no | Referenced entity does not exist. | The referenced entity (transaction, proposal, etc.) does not exist in contract storage. | Check that the identifier was obtained from a successful prior call; do not retry with the same id. |
| 3 | `InvalidInput` | `Validation` | no | no | Arguments failed validation. | One or more arguments failed the contract's validation rules (e.g. empty string, out-of-range amount). | Inspect the argument values, correct the offending field, and resubmit. |
| 4 | `AlreadyExists` | `Validation` | no | no | Entity already present; drop the call. | An entity with the given identifier is already present in storage. | Drop the call; the desired end-state already exists. |
| 5 | `InsufficientFunds` | `Funds` | no | yes | Balance too low to satisfy the operation. | The operation requires more balance than is currently available. | Top up the relevant balance before retrying. |
| 6 | `DisputeClosed` | `State` | no | no | Operation attempted against a closed dispute. | The targeted dispute has already been resolved and cannot be acted on. | No further action needed; the dispute is already closed. |
| 7 | `StateConflict` | `State` | yes | no | Transient state mismatch; safe to retry. | The state machine does not allow this transition right now (e.g. a lock is held or a timelock has not elapsed). | Wait for the conflicting condition to resolve, then retry. |
| 8 | `Internal` | `Internal` | no | yes | Invariant violation; requires investigation. | An internal invariant check failed, indicating unexpected contract state. | Alert an operator immediately; do not retry without investigation. |

> The table above is kept in lockstep with the `ContractError` enum by the
> completeness test in `src/lib.rs`, which enumerates every variant and asserts
> that each exposes a code, a category, and the two triage flags. Any new
> variant added to the enum must be reflected here and in that test.

## Error-code budget (Soroban's 50-variant cap)

Soroban caps a `#[contracterror]` enum at **50 cases**. The pre-squash feature
set needed **61** conceptually distinct errors, so `ContractError` is a
*consolidation* of those 61 into ≤50 actual variants. This section is the
canonical mapping from "conceptually distinct error" to "actual variant" and
explains every merge decision. The authoritative decision record lives in
`DECISIONS.md` under the existing "Error-code budget" header; this table is the
operational companion.

### Merge rules

1. **Category is preserved.** Every merge keeps the operationally meaningful
distinction between *invalid input* (`Validation`), *not authorized* (`Auth`),
and *state machine forbids this* (`State`). Off-chain callers can always route
on `ErrorCategory`.
2. **Detail moves to events.** Where two conceptually-different errors share a
variant, the disambiguating detail (which entry point, which transition) is
emitted in the corresponding event, not encoded as a separate variant.
3. **Codes are stable.** A merged variant keeps the code of its primary
concept; the folded concepts are documented below and never renumbered.

### Consolidation map (61 concepts → ≤50 variants)

| Concept(s) folded | Actual variant | Category | Rationale |
| --- | --- | --- | --- |
| `AlreadyInitialized` | `AlreadyExists` | `Validation` | Same "entity already present" category; init is just another entity. |
| `SchemaVersionMismatch`, `InvalidSchemaCompatRange` | `InvalidInput` | `Validation` | Both are caller-supplied schema arguments that failed validation. |
| `CannotCancel`, `AlreadyCancelled` | `StateConflict` | `State` | Both are "the state machine forbids this cancel"; retry-safe. |
| `RetryLimitExceeded` | `StateConflict` | `State` | Ordering/transient condition; retry-safe, same category. |
| `SelfCheckFailed` | `Internal` | `Internal` | Invariant violation; alert-worthy. |
| `TimelockNotElapsed` | `StateConflict` | `State` | Ordering condition; retry-safe once the timelock passes. |
| `NoPendingChange`, `NothingToRollback` | `NotFound` | `Lookup` | Nothing to act on; drop the call. |
| `UnknownMigration` | `NotFound` | `Lookup` | Referenced migration does not exist. |
| `MigrationFailed` | `Internal` | `Internal` | Invariant violation during migration; alert-worthy. |
| `UpgradeNotReversible` | `StateConflict` | `State` | State machine forbids the upgrade; not retry-safe. |

All other concepts map 1:1 to their own variant. The result is a single
`ContractError` enum with **≤50 variants**, asserted at compile time by the
variant-count guard in `src/types.rs` so a future PR cannot silently regress
past the cap.

## Coordination

This document describes the contract-side structured-error surface only. The
actual off-chain error-handling and alerting logic that consumes it lives in
`synapse-core` and is owned by the relay service team; wiring the relay's
triage logic to `code()` / `context()` is a direct coordination item for that
team.
