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

Every event is published with **two** topics and a typed struct payload
(see [`EVENTS.md` §1](../EVENTS.md#1-wire-format)):

```text
topics: [ Symbol("synapse"), Symbol("<event_name>") ]
data:   #[contracttype] struct, encoded as an XDR map keyed by field name
```

Filter on `topics[0] == "synapse"` and dispatch on `topics[1]`. The full list of
`topics[1]` values, with the struct each one carries, is the
[`EVENTS.md` §2 table](../EVENTS.md#2-implementation-status). That table is
machine-checked against `src/events.rs` in CI
(`event_decoder_covers_catalogued_topics`), so it is the list a decoder must
cover.

> If `src/events.rs` gains a new event, add a decoder arm in section 2. A
> missing arm is a completeness bug, not a nice-to-have.

## 2. Reference decoder (Rust)

The module below is a standalone reference, written against the structs in
`src/events.rs`. The crate is a `cdylib` and its `events` module is private, so
copy the `#[contracttype]` structs you need into your indexer (field names,
types and order must match exactly) and add one arm per topic in the §2 table.
Only a few arms are shown here.

```rust
use soroban_sdk::{Env, Symbol, TryFromVal, Val, Vec};

// Copied verbatim from src/events.rs.
use crate::synapse_events::{
    EventStatusChanged, EventTransactionCompleted, EventTransactionRegistered,
};

#[derive(Debug)]
pub enum ContractEvent {
    Registered(EventTransactionRegistered),
    StatusChanged(EventStatusChanged),
    Completed(EventTransactionCompleted),
    // … one variant per topic in EVENTS.md §2
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Not a Synapse event (`topics[0] != "synapse"` or fewer than 2 topics).
    NotSynapse,
    /// `topics[1]` is not in EVENTS.md §2.
    UnknownTopic(Symbol),
    /// The payload did not match the topic's struct.
    MalformedPayload(Symbol),
}

/// Decode one event from `(topics, data)` as returned by
/// `Env::events().all()` or the RPC `getEvents` response.
///
/// Returns an error instead of panicking so one malformed event cannot take
/// down a long-running poller.
pub fn decode(env: &Env, topics: &Vec<Val>, data: Val) -> Result<ContractEvent, DecodeError> {
    let symbol = |i| {
        topics
            .get(i)
            .and_then(|t| Symbol::try_from_val(env, &t).ok())
            .ok_or(DecodeError::NotSynapse)
    };
    if symbol(0)? != Symbol::new(env, "synapse") {
        return Err(DecodeError::NotSynapse);
    }
    let name = symbol(1)?;
    let malformed = |_| DecodeError::MalformedPayload(name.clone());

    if name == Symbol::new(env, "reg") {
        EventTransactionRegistered::try_from_val(env, &data)
            .map(ContractEvent::Registered)
            .map_err(malformed)
    } else if name == Symbol::new(env, "status") {
        EventStatusChanged::try_from_val(env, &data)
            .map(ContractEvent::StatusChanged)
            .map_err(malformed)
    } else if name == Symbol::new(env, "done") {
        EventTransactionCompleted::try_from_val(env, &data)
            .map(ContractEvent::Completed)
            .map_err(malformed)
    } else {
        Err(DecodeError::UnknownTopic(name))
    }
}
```

Two topic pairs share a struct: `rs_prop` / `rs_canc` both carry
`EventRelaySignerChange`, and `rs_add` / `rs_rm` both carry
`EventRelaySignerMembership`. Dispatch on the topic, not the payload shape.

### Validating against a real testnet deployment

Synthetic fixtures are not sufficient — decode drift is exactly the bug class
this module exists to prevent. Before merging a change to `src/events.rs`:

1. Deploy the contract to testnet and exercise the events you decode.
2. Fetch the raw events via the RPC `getEvents` endpoint (or
   `Env::events().all()` in a test).
3. Feed each raw `(topics, data)` pair through [`decode`] and assert the result
   equals the expected [`ContractEvent`].
4. Record the testnet transaction hashes in the PR description so a reviewer can
   reproduce the decode against the same real events.

### Keeping the decoder in sync (CI)

CI runs `event_decoder_covers_catalogued_topics`, which fails if the
EVENTS.md §2 table and `src/events.rs` disagree on any topic or emitter. That
keeps the *index* of topics honest; your decoder still needs an arm per topic.
For this guide, a cheap extra guard is:

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

- **Topics.** `topics[0]` is always `"synapse"` and `topics[1]` is the event
  name. In `@stellar/stellar-sdk` each arrives as an `xdr.ScVal`; convert with
  `scValToNative` and compare against the names in the EVENTS.md §2 table.
- **Payloads.** The payload is a `#[contracttype]` struct, encoded as an XDR
  map. `scValToNative(event.value)` yields a plain object keyed by the field
  names in EVENTS.md §3. Read fields by name.
- **Addresses.** `scValToNative` returns the `G...`/`C...` string form; keep it
  as a string rather than re-wrapping it.
- **Unknown topics.** Mirror the Rust `UnknownTopic` arm: log and skip, never
  throw, so a newly added event cannot crash the poller.

```ts
// Mirror of the Rust `decode` above. Keep arms in the same order.
function decode(event: { topic: xdr.ScVal[]; value: xdr.ScVal }) {
  if (event.topic.length < 2 || scValToNative(event.topic[0]) !== "synapse") {
    return { type: "not_synapse" };
  }
  const name = scValToNative(event.topic[1]);
  const data = scValToNative(event.value);
  switch (name) {
    case "reg":    return { type: "registered", txId: data.tx_id, amount: data.amount };
    case "status": return { type: "status", txId: data.tx_id, from: data.old_status, to: data.new_status };
    case "done":   return { type: "completed", txId: data.tx_id, hash: data.stellar_tx_hash };
    default:       return { type: "unknown", name, data };
  }
}
```

## 4. Topic-filtering guidance

Subscribe to the narrowest set of topics your use case needs. Every Synapse
event shares `topics[0] = "synapse"`, so filter on `topics[1]`.

| Use case | Subscribe to | Rationale |
| --- | --- | --- |
| Phase 2 Swap Engine | `status` (with `new_status == Completed`), `fwd` | Settled deposits and explicit forwarding intents. |
| Transaction indexer | `reg`, `batch`, `status`, `done`, `partial`, `fail`, `cancel`, `retry`, `merged` | The full transaction lifecycle. |
| Security / ops alerting | `admin`, `propose`, `relay`, `rs_*`, `pause`, `upgrade`, `up_*`, `rollback`, `migrate`, `chk_fail`, `slashed` | Privilege, signer-set, pause and code changes. |
| Collateral / fee accounting | `bonded`, `unbondrq`, `unbondcl`, `slashed`, `tierset`, `rebate`, `param` | Wave 2 economics. |

**Ordering.** Within one invocation, events are emitted in the order listed in
[EVENTS.md §4](../EVENTS.md#4-additive-trailing-field-pattern-required-convention).
`getEvents` returns them in ledger order; use the ledger sequence and the
event's paging token for global ordering.

**Finality.** Stellar has no reorgs: an event in a closed ledger is final. See
[`event-finality.md`](./event-finality.md) for polling, cursor recovery and
retention windows.

## 5. Completeness checklist

Before merging any change that touches events:

- [ ] Every topic in the `EVENTS.md` §2 table has a decoder arm.
- [ ] Every event has exactly one arm in the Rust `decode` (section 2).
- [ ] The TypeScript port (section 3) mirrors the Rust arms one-to-one.
- [ ] The decoder was validated against real testnet events, with tx hashes in
      the PR description.
- [ ] `event_decoder_covers_catalogued_topics` passes.
