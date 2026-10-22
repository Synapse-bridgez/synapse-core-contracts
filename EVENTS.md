# Event Ordering Guarantees

This document records the guaranteed emission order for every entry point that emits **two or more** events in a single invocation. Entry points that only ever emit a single event make no ordering claim and are not listed here.

## Multi-Event Entry Points

| Entry point | Events emitted | Guaranteed order |
| --- | --- | --- |
| `complete_transaction` | `status`, `done` | `status` is emitted **before** `done`. |
| `start_processing` | `status` | Single `status` event emitted on the `Pending → Processing` transition. |
| `cancel_transaction` | `status`, `cancel` | `status` is emitted **before** `cancel`. |
| `register_callback` | `reg` | Single `reg` event emitted per registered callback. |
| `batch_register_callback` | one `reg` per item, then one `batch` summary | All per-item `reg` events are emitted **in input order**, followed by exactly one `batch` summary event **last**. |
| upgrade finalize | preceding upgrade events, then `up_fin` | `up_fin` is emitted **after** all preceding upgrade events. |
| dispute resolution | `dispute_raised`, `dispute_resolved` | `dispute_raised` is emitted **before** `dispute_resolved`. |

## Notes

- **`complete_transaction`** — the `status`-then-`done` ordering is a preserved, verified guarantee. It is asserted by a dedicated ordering test (asserting the events vector's order, not merely presence) and must not be reordered.
- **`start_processing`** — emits a single `status` event on the `Pending → Processing` transition; no ordering claim applies.
- **`cancel_transaction`** — the `status`-then-`cancel` ordering is a preserved guarantee. `cancel` is the terminal event of the `Cancelled` branch (a documented second terminal branch alongside `Failed`), reachable only from `Pending`/`Processing`.
- **`register_callback`** — emits a single `reg` event per registered callback; no ordering claim applies.
- **`batch_register_callback`** — per-item `reg` events follow the input order of the batch; the single `batch` summary is always emitted last so consumers can treat it as a completion marker.
- **Upgrade finalize** — `up_fin` is the terminal event of the flow and is emitted only after all preceding upgrade events.
- **Dispute resolution** — `dispute_raised` precedes `dispute_resolved` within the same resolution flow.

## Single-Event Entry Points

Entry points that emit only a single event require no ordering claim and are intentionally omitted from the table above. Ordering guarantees are only meaningful when two or more events are emitted in one invocation.

## Transaction Lifecycle Terminal Branches

The transaction state machine has two terminal branches:

- **`Failed`** — reachable from any non-terminal state via `fail_transaction`.
- **`Cancelled`** — reachable only from `Pending`/`Processing` via `cancel_transaction`; emits the `cancel` event. `Completed`/`Failed` are rejected with `CannotCancel`, and an already-`Cancelled` transaction is rejected with `AlreadyCancelled`.

## Verification

Each multi-event entry point above has a dedicated ordering-assertion test that checks the position of events in the emitted events vector. These tests are collected and verified together to keep this document in sync with the contract's behavior.