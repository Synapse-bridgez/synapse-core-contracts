# ADR-0008 — Fee accrual and two-party treasury withdrawal

> **Status:** Proposed
> **Date:** 2026-09-29
> **Deciders:** Synapse Bridge core team, contract author (maintainer sign-off pending)

---

## Context

Until now the contract tracked no economic value: fees, if any, were
calculated and tracked entirely off-chain. Issues #141 and #142 introduce an
on-chain fee and treasury, which is a trust-model expansion. The contract now
holds a balance that someone may try to inflate, drain, or corrupt.

Two platform facts shape the design:

- Transaction `amount` is an `i128` bounded only below (`> 0`), so any
  `amount * rate` product can overflow at the top of the range.
- The contract holds no tokens. `amount` is an accounting record of a Stellar
  payment settled elsewhere. The treasury balance is likewise an **accounting
  ledger**: `authorize_withdrawal` debits it and emits an event, and the actual
  token movement happens off-chain against that record.

At the time of writing, no N-of-M signer set exists in the contract. The
access-control bucket that would supply one has not landed. The two roles
available are the admin, which R-02 requires to be a ≥3-of-5 multisig, and
the relay signer.

---

## Options considered

### Fee calculation

**Option A: `floor(amount * fee_bps / 10_000)` computed as a single product.**
This is simple, but the product overflows for large amounts. The call would
then have to fail, which means a legitimate settlement could never complete.

**Option B: the same floor value computed overflow-free (chosen).**
With `amount = q * 10_000 + r`, the fee is `q * bps + floor(r * bps / 10_000)`.
The result is identical to Option A wherever Option A does not overflow, and it
is exact for every positive `i128`, because `q * bps ≤ amount` and
`r * bps < 10^8`. Checked ops remain as defence in depth.

Rounding is always **down**, so the protocol never over-charges. Rates run
from 0 to 10 000 bps, and 0 disables accrual. The rate is read at completion
time, so a later rate change never alters fees that have already accrued.

### Where the fee rate and limits live

**Option A: dedicated storage keys and setters (`set_fee_bps`,
`configure_treasury`, extra `initialize` arguments).**
This is explicit and typed. However, it adds a second configuration path
beside the param registry (#146), and it is a breaking `initialize` change.

**Option B: registry params (chosen).**
The registry's own contract is that all tunable values (fee rate, unbond
delay, slash percentage, fee ceiling, …) live there rather than as
independent admin-settable fields, and it already reserves `base_fee_bps`.
The contract reads `base_fee_bps`, `treasury_epoch_cap`, and
`treasury_epoch_length`. `set_param` range-checks these three names
(`InvalidParamValue`), and changes are observable as `param` events.
`initialize` is unchanged.

An unset `base_fee_bps` means no fee. The treasury counts as configured only
when both epoch params are set.

### Treasury overflow

The treasury uses `checked_add`, and an overflow returns `ArithmeticOverflow`.
That rejects the completion, and Soroban rolls back the whole call. Reaching
this needs cumulative fees near `i128::MAX` (~1.7 × 10^38 stroops), which is
economically unreachable. Failing closed is preferred over saturating, because
saturating would silently lose accounting.

### Withdrawal authorization

**Option A: single admin call.**
This was rejected. #142 exists specifically to avoid one key draining the
treasury.

**Option B: N-of-M signer quorum.**
This is the eventual target, but it depends on an access-control primitive the
contract does not have yet.

**Option C: admin proposes, relay signer co-authorizes (chosen).**
There are two independent keys held by different parties. The admin side is
itself a multisig per R-02. The relay's `authorize_withdrawal(caller, amount,
destination)` restates the exact proposal, and those arguments are covered by
the relay's `require_auth()`. A compromised admin who replaces the proposal
after the relay has reviewed it therefore gets `WithdrawalProposalMismatch`
rather than a silent substitution.

### Per-epoch cap

A `TreasuryConfig { epoch_cap, epoch_length }` bounds the total withdrawn per
window of `epoch_length` ledgers, independently of authorization. This is
defence in depth against a colluding or doubly compromised admin + relay. The
epoch opens at the first withdrawal after the previous one expires, and resets
when `now >= epoch_start + epoch_length`. A zero `epoch_length` is rejected
because it would reset on every call. Exceeding the balance
(`InsufficientTreasuryBalance`) and exceeding the cap (`WithdrawalCapExceeded`)
are distinct errors.

---

## Decision

Adopt the overflow-free fee formula, fail-closed treasury overflow,
registry-held parameters, two-party withdrawal (Option C), and a per-epoch
cap. The new admin entry point is `propose_withdrawal`, and the new relay entry
point is `authorize_withdrawal`. The read-only queries are `treasury_balance`,
`treasury_config` (derived from the params), and `pending_withdrawal`.

---

## Consequences

- **Positive:** Fee accounting is on-chain, auditable through `fee`, `param`,
  `wprop`, and `wexec` events, and immune to arithmetic wrap. No single key can
  move treasury value, and even two cooperating keys are bounded per epoch.
- **Negative / accepted trade-offs:**
  - The relay signer, a hot key (R-01), is one of the two withdrawal parties.
    Tracked as THREAT_MODEL.md **R-06**.
  - The admin sets the epoch cap, so a compromised admin could raise it.
    That change emits a `param` event, and each withdrawal still needs the
    relay's co-signature.
  - Fee accrual does not apply the per-anchor rebate (#145), because a
    `Transaction` carries no anchor `Address` to look up a tier by.
  - `complete_transaction` may now emit a third event (`fee`) after `done`.
- **Follow-up work:**
  - Replace relay co-authorization with the N-of-M quorum once the
    access-control bucket lands (supersede this ADR).
  - A minimum-notice delay for parameter changes (#150).
  - Apply the anchor rebate at accrual once transactions record their anchor.
  - Accrued-vs-withdrawn reconciliation query (#148). Fee-distribution
    splitting (#147).

---

## References

- Code: `src/lib.rs` (`complete_transaction`, `compute_fee`,
  `propose_withdrawal`, `authorize_withdrawal`), `src/storage.rs`.
- Tests: `src/test_fee_treasury.rs`, `src/test_accepted_risks.rs` (R-06).
- [`EVENTS.md`](../../EVENTS.md): `fee`, `wprop`, `wexec` (and `param`).
- [`THREAT_MODEL.md`](../../THREAT_MODEL.md) §4.8, §8 R-06.
