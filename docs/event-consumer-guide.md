# Event Consumer Guide

This guide is the companion to [`EVENTS.md`](../EVENTS.md). `EVENTS.md` is the
locked, prose-and-table schema for every event the contract emits; this document
shows how to *consume* those events correctly, including a reference decoder and
topic-filtering guidance for common use cases.

> **Why this exists.** Every subscriber (synapse-web's event poller today, and
> eventually Phase 2/3 contracts and their own off-chain services) previously had
> to independently translate `EVENTS.md`'s prose into working decode logic. That
> is a reliable source of "subscriber misread the docs" bugs. The reference
> decoder below is the single, tested translation of the schema into code.

## 1. Topic layout

Every event is emitted with a single indexed topic: the event name as a
`Symbol`. Payloads are carried in the data section as a tuple of the fields
documented in `EVENTS.md`. There are no anonymous events and no multi-topic
indexing, so a subscriber only ever needs to match on `topics[0]`.

| Event name (topic) | Payload (data) |
| --- | --- |
| `init` | `(admin: Address)` |
| `batch` | `(batch_id: u64, count: u32)` |
| `admin` | `(admin: Address, action: Symbol)` |

> Keep this table in sync with `EVENTS.md`. If `src/events.rs` gains a new
> event, add a row here **and** a decoder arm in section 2 — a missing arm is a
> completeness bug, not a nice-to-have.

## 2. Reference decoder (Rust)

The module below is a standalone reference. It is intentionally dependency-free
beyond `soroban-sdk` so it can be dropped into any off-chain Rust service, and
it is written to mirror `src/events.rs` one-to-one.

```rust
//! Reference decoder for the contract's event stream.
//!
//! This module is the executable form of `EVENTS.md`. It is deliberately
//! standalone: copy it into your indexer rather than re-deriving the schema
//! from prose. Every catalogued event has exactly one arm in [`decode`].

use soroban_sdk::{Address, Env, Symbol, TryFromVal, Val};

/// A decoded contract event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractEvent {
    /// Emitted once at initialization. Payload: `(admin)`.
    Init { admin: Address },
    /// Emitted per batch write. Payload: `(batch_id, count)`.
    Batch { batch_id: u64, count: u32 },
    /// Emitted on every admin action. Payload: `(admin, action)`.
    Admin { admin: Address, action: Symbol },
}

/// Error returned when an event cannot be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// `topics` was empty; every contract event has a name topic.
    MissingTopic,
    /// The topic did not match any catalogued event name.
    UnknownTopic(Symbol),
    /// The data section did not match the topic's documented payload.
    MalformedPayload(Symbol),
}

/// Decode a single event from its raw `(topics, data)` representation.
///
/// `topics` is the indexed topic list and `data` is the payload value, exactly
/// as delivered by `Env::events().all()` / the RPC `getEvents` response.
///
/// Returns [`DecodeError`] rather than panicking so a single malformed event
/// cannot take down a long-running poller.
pub fn decode(
    env: &Env,
    topics: &[Val],
    data: Val,
) -> Result<ContractEvent, DecodeError> {
    let name = topics
        .first()
        .ok_or(DecodeError::MissingTopic)
        .and_then(|t| Symbol::try_from_val(env, t).map_err(|_| DecodeError::MissingTopic))?;

    // Match on the event name. Keep arms in the same order as EVENTS.md.
    if name == Symbol::new(env, "init") {
        let (admin,): (Address,) =
            TryFromVal::try_from_val(env, &data).map_err(|_| DecodeError::MalformedPayload(name))?;
        Ok(ContractEvent::Init { admin })
    } else if name == Symbol::new(env, "batch") {
        let (batch_id, count): (u64, u32) =
            TryFromVal::try_from_val(env, &data).map_err(|_| DecodeError::MalformedPayload(name))?;
        Ok(ContractEvent::Batch { batch_id, count })
    } else if name == Symbol::new(env, "admin") {
        let (admin, action): (Address, Symbol) =
            TryFromVal::try_from_val(env, &data).map_err(|_| DecodeError::MalformedPayload(name))?;
        Ok(ContractEvent::Admin { admin, action })
    } else {
        Err(DecodeError::UnknownTopic(name))
    }
}
```

### Validating against a real testnet deployment

Synthetic fixtures are not sufficient — decode drift is exactly the bug class
this module exists to prevent. Before merging a change to `src/events.rs`:

1. Deploy the contract to testnet and exercise each event (init, a batch write,
   and an admin action).
2. Fetch the raw events via the RPC `getEvents` endpoint (or
   `Env::events().all()` in a test).
3. Feed each raw `(topics, data)` pair through [`decode`] and assert the result
   equals the expected [`ContractEvent`].
4. Record the testnet transaction hashes in the PR description so a reviewer can
   reproduce the decode against the same real events.

### Keeping the decoder in sync (CI)

Add a CI check that fails whenever `src/events.rs` changes without a
corresponding update to this guide. A minimal guard:

```sh
# Fail if events.rs changed but the consumer guide did not.
if git diff --name-only "$BASE"...HEAD | grep -q '^src/events.rs$' \
   && ! git diff --name-only "$BASE"...HEAD | grep -q '^docs/event-consumer-guide.md$'; then
  echo "src/events.rs changed; update docs/event-consumer-guide.md" >&2
  exit 1
fi
```

At minimum, document this as a manual verification step in the PR checklist.

## 3. Porting to TypeScript (for `synapse-web`)

`synapse-web`'s event poller is the existing consumer, so the Rust module above
is the source of truth and the TypeScript port must mirror it arm-for-arm.

- **Topics.** `topics[0]` is the event name. In `@stellar/stellar-sdk` it
  arrives as an `xdr.ScVal`; convert with `scValToNative` and compare against the
  string names (`"init"`, `"batch"`, `"admin"`).
- **Payloads.** `scValToNative(event.value)` yields a JS array matching the
  documented tuple order. Destructure positionally — do **not** rely on object
  keys, since the payload is a tuple, not a map.
- **Addresses.** `scValToNative` returns the `G...`/`C...` string form; keep it
  as a string rather than re-wrapping it.
- **Unknown topics.** Mirror the Rust `UnknownTopic` arm: log and skip, never
  throw, so a future event added by a Phase 2/3 contract cannot crash the poller.

```ts
// Mirror of the Rust `decode` above. Keep arms in the same order.
function decode(event: { topic: xdr.ScVal[]; value: xdr.ScVal }) {
  const name = scValToNative(event.topic[0]);
  const data = scValToNative(event.value);
  switch (name) {
    case "init":  return { type: "init",  admin: data[0] };
    case "batch": return { type: "batch", batchId: data[0], count: data[1] };
    case "admin": return { type: "admin", admin: data[0], action: data[1] };
    default:      return { type: "unknown", name, data };
  }
}
```

## 4. Topic-filtering guidance

Subscribe to the narrowest set of topics your use case needs. Filtering on
`topics[0]` is cheap and keeps poller load proportional to relevance.

| Use case | Subscribe to | Rationale |
| --- | --- | --- |
| Admin/ops dashboard | `init`, `admin` | Only needs lifecycle and privileged actions. |
| Data indexer / analytics | `batch` | Batch writes are the only data-bearing events. |
| Full mirror / audit log | `init`, `batch`, `admin` | Complete history; required for reconciliation. |
| Alerting on privilege changes | `admin` | Admin actions are the only security-relevant events. |
| Bootstrap / first-run sync | `init` | Confirms the deployment's initial admin. |

**Ordering.** Events are emitted in the order the contract executes them, and
`getEvents` returns them in ledger order. Do not assume cross-ledger ordering
beyond what the ledger sequence provides; use `batch_id` for batch ordering and
the ledger sequence for global ordering.

**Reorgs.** Treat events as final only after the ledger is confirmed. On a
reorg, re-fetch from the last confirmed ledger rather than patching in place.

## 5. Completeness checklist

Before merging any change that touches events:

- [ ] Every event in `EVENTS.md` has a row in section 1.
- [ ] Every event has exactly one arm in the Rust `decode` (section 2).
- [ ] The TypeScript port (section 3) mirrors the Rust arms one-to-one.
- [ ] The decoder was validated against real testnet events, with tx hashes in
      the PR description.
- [ ] The CI sync guard (section 2) passes.
