# Contract Error Codes

This document is the companion reference for the structured error surface
exposed by `ContractError` in `src/types.rs`. It enumerates every error code
together with its intended off-chain handling guidance so a relay service can
triage failed calls without pattern-matching on human-readable strings.

## Soroban error shape

`ContractError` is a `#[contracterror]` enum: every variant is a stable numeric
code (its `#[repr(u32)]` discriminant) exposed to callers as a Soroban
contract error. Callers dispatch on the code; human-readable detail that does
not fit in a code (e.g. a supplied count) is emitted in an event, not encoded
in the error.

## Error code reference

| Code | Variant | Category | Retry-safe | Alert-worthy | Notes |
| --- | --- | --- | --- | --- | --- |
| 1 | `ZeroAmount` | Validation | no | no | A supplied amount was zero or non-positive. Covers the transfer (`register_callback`) and treasury-withdrawal paths so a client can explain why a transfer failed (issue #472). |
| 2 | `AmountExceedsCeiling` | Validation | no | no | The amount exceeded the resolved ceiling. |
| 3 | `TagLimitExceeded` | Validation | no | no | More than `MAX_TAGS_PER_TRANSACTION` tags were supplied. The supplied count and the limit are emitted in a `taglimit` event so a client can render "you sent N, limit is M" (issue #471). |
| 10 | `NotAdmin` | Auth | no | yes | `require_admin` failed: caller is authenticated but is not the admin (issue #469). |
| 11 | `NotRelayOrAdmin` | Auth | no | yes | `require_relay_or_admin` failed: caller matches neither the relay signer nor the admin (issue #469). |
| 12 | `NotPendingAdmin` | Auth | no | yes | `require_pending_admin` failed: caller is not the pending admin (issue #469). |
| 13 | `NotGuardian` | Auth | no | yes | `require_guardian` failed: caller is not the emergency guardian (issue #469). |
| 20 | `ContractPaused` | State | no | no | The contract is paused; the call was rejected. Use `get_pause_state` (issue #470) to discover this before submitting. |
| 21 | `AlreadyExists` | State | no | no | A referenced entity already exists (e.g. duplicate transaction id). |
| 22 | `NotFound` | Lookup | no | no | A referenced entity was not found. |
| 23 | `NotInitialised` | State | no | no | The contract has not been initialised. |
| 24 | `InsufficientTreasuryBalance` | Funds | no | yes | The treasury balance is too low for the withdrawal. |

## Issue coverage

The four issues this contract implements map onto the codes above:

- **#469** — distinct, documented admin-guard codes: `NotAdmin`, `NotRelayOrAdmin`,
  `NotPendingAdmin`, `NotGuardian` (replacing a single generic auth error).
- **#470** — read-only `get_pause_state` query so callers can read the pause
  flag and reason before submitting; `ContractPaused` is the rejection code.
- **#471** — `TagLimitExceeded` carries no count itself (Soroban errors are
  codes, not data); the supplied count and the limit travel in the `taglimit`
  event emitted alongside the error.
- **#472** — `ZeroAmount` is the dedicated, documented code for zero-amount
  rejections on both the transfer and treasury-withdrawal paths.

## Error-code budget

Soroban caps a `#[contracterror]` enum at 50 variants. This contract stays well
within that budget (12 variants); new variants must continue to fit and must be
added to this table in the same change.
