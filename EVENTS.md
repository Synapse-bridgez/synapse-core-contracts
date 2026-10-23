# Events

This document is the **locked event schema** for the contract. It is the single
source of truth for the events the contract emits, and it is checked against the
implementation by the automated conformance test (issues #43/#44). Any change to
an emitted event — its topic, its field list, or its emitting entry point — must
be reflected here in the same PR, or the conformance check will fail.

The schema is "locked" in the sense that it is authoritative and machine-checked:
it is not a wish list of events we would like to have. Only events that have a
real, working emitter in `src/events.rs` and a corresponding `#[contractimpl]`
entry point are catalogued below.

## 1. Conventions

- **Topic** — the symbol under which the event is published. Topics are short,
  stable identifiers; they are part of the public ABI and must not be renamed
  without a coordinated off-chain migration.
- **Fields** — the `#[contracttype]` payload struct for the event, listed in
  declaration order with the field name and its Soroban type. Field order is
  significant: it is the serialization order of the payload.
- **Emitter** — the `#[contractimpl]` entry point that publishes the event.
- **Ordering** — where a single entry point publishes more than one event, the
  guaranteed emission order is stated explicitly (issue #45). Off-chain
  consumers may rely on this ordering.

## 2. Event catalogue

Each row below corresponds to exactly one event with a working emitter. The
original ten "live emitters" from the README (`init`, `reg`, `status`, `done`,
`fail`, `propose`, `admin`, `relay`, `pause`, `upgrade`) are all present, plus
any events restored by issues #32–#40.

| Topic | Fields (`#[contracttype]`, declaration order) | Emitting entry point | Guaranteed ordering |
| --- | --- | --- | --- |
| `init` | `admin: Address`, `version: u32` | `initialize` | — |
| `reg` | `account: Address`, `role: u32` | `register` | — |
| `status` | `id: u64`, `status: u32` | `update_status`, `complete_transaction` | In `complete_transaction`: `status` is emitted **before** `done`. |
| `done` | `id: u64`, `result: Bytes` | `complete_transaction` | In `complete_transaction`: emitted **after** `status`. |
| `fail` | `id: u64`, `reason: Symbol` | `fail_transaction` | — |
| `propose` | `id: u64`, `proposer: Address`, `payload: Bytes` | `propose` | — |
| `admin` | `admin: Address`, `action: Symbol` | `set_admin` | — |
| `relay` | `id: u64`, `relayer: Address` | `relay` | — |
| `pause` | `paused: bool`, `by: Address` | `pause`, `unpause` | — |
| `upgrade` | `version: u32`, `by: Address` | `upgrade` | — |

> **Note on ordering (issue #45):** the only entry point that publishes more
> than one event is `complete_transaction`, which emits `status` followed by
> `done`. All other entry points publish exactly one event, so no ordering
> guarantee applies beyond the single emission.

## 3. Explicitly excluded topics

The following eight topics were **removed** in commit `5f66065` because they
were never emitted by any code path:

- `expire`
- `uqset`
- `uprop`
- `attest`
- `renounce`
- `guardian`
- `apause`
- `aunpause`

These topics are **excluded from the locked schema**. They must not be
reintroduced merely because they once appeared in an earlier version of this
document. A topic from this list may only be added back if a specific issue in
this wave demonstrably restores a real emitter for it — that is, a
`#[contracttype]` payload, a publish call in `src/events.rs`, and a
`#[contractimpl]` entry point that triggers it. Absent all three, the topic
stays out.

## 4. Cross-check against README

The README's "Live emitters today" list is:

`init`, `reg`, `status`, `done`, `fail`, `propose`, `admin`, `relay`, `pause`,
`upgrade`

Every one of these ten topics appears in the §2 catalogue above. Any event
added by issues #32–#40 is catalogued alongside them. If the README list and
this catalogue ever disagree, this file — as validated by the conformance test
against `src/events.rs` — is authoritative, and the README must be updated to
match.

## 5. Maintenance

- Add or change an event only together with its emitter in `src/events.rs` and
  its entry point in the `#[contractimpl]` block.
- Update this catalogue in the same PR. The conformance test (issues #43/#44)
  compares the §2 rows against the actual emitters and fails on any drift.
- Do not add speculative or "planned" events. This document describes what the
  contract emits today, not what it might emit later.

## 6. Event Ordering Guarantees

This section records the guaranteed emission order for every entry point that
emits **two or more** events in a single invocation. Entry points that only ever
emit a single event make no ordering claim and are not listed here.

### Multi-Event Entry Points

| Entry point | Events emitted | Guaranteed order |
| --- | --- | --- |
| `complete_transaction` | `status`, `done` | `status` is emitted **before** `done`. |
| `start_processing` | `status` | Single `status` event emitted on the `Pending → Processing` transition. |
| `cancel_transaction` | `status`, `cancel` | `status` is emitted **before** `cancel`. |
| `register_callback` | `reg` | Single `reg` event emitted per registered callback. |
| `batch_register_callback` | one `reg` per item, then one `batch` summary | All per-item `reg` events are emitted **in input order**, followed by exactly one `batch` summary event **last**. |
| upgrade finalize | preceding upgrade events, then `up_fin` | `up_fin` is emitted **after** all preceding upgrade events. |
| dispute resolution | `dispute_raised`, `dispute_resolved` | `dispute_raised` is emitted **before** `dispute_resolved`. |

### Notes

- **`complete_transaction`** — the `status`-then-`done` ordering is a preserved, verified guarantee. It is asserted by a dedicated ordering test (asserting the events vector's order, not merely presence) and must not be reordered.
- **`start_processing`** — emits a single `status` event on the `Pending → Processing` transition; no ordering claim applies.
- **`cancel_transaction`** — the `status`-then-`cancel` ordering is a preserved guarantee. `cancel` is the terminal event of the `Cancelled` branch (a documented second terminal branch alongside `Failed`), reachable only from `Pending`/`Processing`.
- **`register_callback`** — emits a single `reg` event per registered callback; no ordering claim applies.
- **`batch_register_callback`** — per-item `reg` events follow the input order of the batch; the single `batch` summary is always emitted last so consumers can treat it as a completion marker.
- **Upgrade finalize** — `up_fin` is the terminal event of the flow and is emitted only after all preceding upgrade events.
- **Dispute resolution** — `dispute_raised` precedes `dispute_resolved` within the same resolution flow.

### Single-Event Entry Points

Entry points that emit only a single event require no ordering claim and are
intentionally omitted from the table above. Ordering guarantees are only
meaningful when two or more events are emitted in one invocation.

## Transaction Lifecycle Terminal Branches

The transaction state machine has two terminal branches:

- **`Failed`** — reachable from any non-terminal state via `fail_transaction`.
- **`Cancelled`** — reachable only from `Pending`/`Processing` via `cancel_transaction`; emits the `cancel` event. `Completed`/`Failed` are rejected with `CannotCancel`, and an already-`Cancelled` transaction is rejected with `AlreadyCancelled`.

### Verification

Each multi-event entry point above has a dedicated ordering-assertion test that
checks the position of events in the emitted events vector. These tests are
collected and verified together to keep this document in sync with the
contract's behavior.

