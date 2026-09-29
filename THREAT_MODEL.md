# Threat Model

This document describes the actor model, trust boundaries, and accepted risks for the
Soroban smart contract in this repository. It is a living document: every Wave that
introduces new economic logic or new trust assumptions must update it.

## Actors

- **Admin** — holds the admin key; can configure fees, treasury, and pause switches.
- **Treasury** — the destination account for accrued protocol fees.
- **Users / Payers** — accounts that submit transactions and pay fees.
- **Contributors / Bonders** — accounts that bond collateral and may be slashed.
- **Rebate recipients** — accounts that receive rebates from accrued fees.

## Trust Boundaries

- The admin key is trusted for configuration but **not** for arithmetic correctness.
- All on-chain value tracking (fee accrual, treasury withdrawal, bonding/unbonding,
  slashing, rebates, splits, reconciliation) crosses a trust boundary: inputs are
  attacker-influenced and must be validated before use.
- Storage reads are treated as untrusted inputs for arithmetic purposes; a corrupted or
  unexpectedly large stored value must not be able to wrap into a valid-looking result.

## Economic-Logic Attack Surface (this Wave)

This Wave introduces real, meaningful on-chain value tracking for the first time. The
new economic-logic code paths are:

- Fee accrual and fee splits
- Treasury withdrawal
- Bonding / unbonding
- Slashing
- Rebates
- Reconciliation

### Compensating Control: Explicit Checked Arithmetic

**Systematically verified.** Every arithmetic operation on these economic-logic paths
uses explicit `checked_add` / `checked_sub` / `checked_mul` (or equivalent) rather than
relying on the release-profile `overflow-checks = true` setting in `Cargo.toml`.

Rationale:

- The `overflow-checks` flag is a build-profile configuration. It is a useful safety net
  but is not a guarantee that survives every build configuration, and it panics rather
  than returning a recoverable, typed error.
- Explicit checked arithmetic is independent of build-profile configuration and produces
  a deterministic, testable error on overflow/underflow.
- External auditors specifically look for explicit checked arithmetic at economically
  significant call sites when a Wave introduces value tracking.

Handling:

- Overflow/underflow on an economic path returns a typed contract error rather than
  panicking. Callers can observe and react to the failure.
- Boundary-value tests at numeric extremes (values approaching `i128::MAX` and near-zero)
  exercise every audited arithmetic path and assert that overflow/underflow is actually
  caught and handled — not merely that `checked_*` appears textually in the source.

### Residual Risk

- Non-economic arithmetic elsewhere in the contract is **out of scope** for this audit.
  It remains covered by the release-profile `overflow-checks` flag and by the general
  test suite.
- The admin key remains a trusted configuration input; arithmetic hardening does not
  protect against a malicious admin choosing economically hostile (but arithmetically
  valid) parameters. Parameter bounds are handled separately.

## Accepted Risks

- Reliance on the release-profile `overflow-checks` flag for non-economic arithmetic.
- Trust in the admin key for configuration values within documented bounds.

## Review Cadence

This document must be updated whenever a Wave introduces new economic logic, new actors,
or new trust boundaries. The checked-arithmetic control above must be re-verified against
any newly added economic call site.
