# Event Finality & RPC Subscriber Guidance

> **Audience:** synapse-web event poller, Phase 2 Swap Engine, Phase 3 Cross-Chain Bridge,
> any off-chain indexer consuming `synapse-core-contract` events.  
> **Related:** [`EVENTS.md`](../EVENTS.md) — event catalogue and semver policy  
> **Coordination:** synapse-web teams should review §4 before modifying their RPC polling
> loop; flag discrepancies as issues in this repo.

---

## Table of Contents

1. [Stellar Finality Model](#1-stellar-finality-model)
2. [What "Confirmation Depth" Means on Stellar](#2-what-confirmation-depth-means-on-stellar)
3. [The Real Risk: RPC Lag, Not Reorgs](#3-the-real-risk-rpc-lag-not-reorgs)
4. [RPC Subscriber Operational Requirements](#4-rpc-subscriber-operational-requirements)
   - [4.1 Normal polling loop](#41-normal-polling-loop)
   - [4.2 Reconnect and gap recovery](#42-reconnect-and-gap-recovery)
   - [4.3 RPC retention window expiry](#43-rpc-retention-window-expiry)
   - [4.4 Deduplication](#44-deduplication)
5. [Relationship to Contract Idempotency Guarantees](#5-relationship-to-contract-idempotency-guarantees)
6. [Operational Checklist](#6-operational-checklist)
7. [References](#7-references)

---

## 1. Stellar Finality Model

### SCP is BFT: safety over liveness

The Stellar network reaches consensus using the
[Stellar Consensus Protocol (SCP)](https://developers.stellar.org/docs/learn/fundamentals/stellar-consensus-protocol),
a construction of Federated Byzantine Agreement (FBA). SCP is a **Byzantine
Fault Tolerant (BFT)** protocol that explicitly prioritises **safety over
liveness**:

> *"SCP prioritizes fault tolerance and safety over liveness. Because of
> prioritizing safety, blocks can sometimes get stuck while waiting for nodes
> to agree."*
> — [Stellar Docs: SCP](https://developers.stellar.org/docs/learn/fundamentals/stellar-consensus-protocol)

The safety property guarantees that **no two honest nodes ever confirm
different values for the same ledger sequence number**. Once a ledger is
confirmed (closed) by the network, that outcome is mathematically final.

### No rollbacks, no reorgs

Unlike Proof-of-Work chains (Bitcoin, pre-merge Ethereum) — where a longer
competing chain can invalidate previously observed blocks — **Stellar has no
concept of chain reorganisation**. A ledger that has been confirmed cannot be
rolled back. The moment `getEvents` returns an event in a closed ledger, that
event is permanent.

This is a key property that simplifies subscriber design significantly:
**there is no need to wait for multiple ledger confirmations before acting on
an event**.

### Ledger cadence

Ledgers close approximately every **5–6 seconds** on mainnet (average ~5 s;
see the [Stellar Network Dashboard](https://dashboard.stellar.org/)). A
subscriber polling once per ledger can achieve near-real-time event delivery
with a single-digit-second latency.

---

## 2. What "Confirmation Depth" Means on Stellar

On EVM-compatible chains, subscribers typically wait for N block confirmations
before treating an event as final, because a reorg of depth N is possible.
**This concept does not apply to Stellar.**

| Property | Ethereum PoW | Stellar SCP |
|---|---|---|
| Finality mechanism | Probabilistic (longer chain wins) | Deterministic BFT |
| Required confirmation depth | 6–64 blocks | **1 ledger (immediate)** |
| Reorg possible? | Yes | **No** |
| Rollback possible? | Yes (within N blocks) | **No** |

**Practical recommendation:** Treat any event returned by `getEvents` in a
closed ledger as **final at the moment it is returned**. Do not delay action
by waiting for additional ledgers.

The `ledger` field present in every Synapse Core event payload (e.g.,
`EventTransactionCompleted.ledger`, `EventStatusChanged.ledger`) is the ledger
sequence number at which the event was emitted. It is informational and useful
for ordering and audit purposes. It does not represent a "pending" state that
needs further confirmation.

---

## 3. The Real Risk: RPC Lag, Not Reorgs

Since reorgs cannot happen on Stellar, the risk model for subscribers is
different from EVM chains. The risks worth guarding against are:

### Risk 1: RPC node lag / staleness

An RPC node can fall behind the canonical chain tip. If your subscriber is
polling a lagging node, `latestLedger` in the RPC response will be behind the
actual network tip. The subscriber may believe it has seen all events up to
ledger N, when in reality the node has not yet ingested ledgers N+1, N+2, etc.

**Detection:** Every `getEvents` response includes `latestLedger`. Compare
this against an expected cadence — if `latestLedger` has not advanced in more
than ~30 seconds (≈ 5–6 ledger close times), the RPC node is likely stalled
or partitioned.

### Risk 2: Subscriber gap (missed ledgers)

If the subscriber process restarts, crashes, or its network connection to the
RPC node breaks, it will miss events emitted in ledgers during the downtime.
This is not a finality issue; the events exist on-chain. The risk is that the
subscriber's own state diverges from on-chain state because it did not consume
the events in the gap.

**Detection and recovery:** Persist the cursor (last seen event ID or ledger
sequence) durably. On restart, re-query from the last checkpoint.

### Risk 3: RPC retention window expiry

Stellar RPC retains events for a default window of **120,960 ledgers
(≈ 7 days)** from the current tip. If a subscriber is offline for more than
this window, the gap cannot be recovered from RPC alone. The events still
exist on the Stellar ledger (the ledger is permanent), but they are no longer
accessible via the standard `getEvents` method against a default-configured
RPC node.

**Detection:** If `startLedger < oldestLedger` in the response, the RPC node
returns an error. This is a hard signal that the gap recovery window has been
exceeded.

**Recovery:** Historical event archives (e.g., [Hubble](https://developers.stellar.org/docs/data/analytics/hubble),
full-history Stellar Core nodes, or third-party indexers) can supply events
outside the RPC retention window. The subscriber must have an out-of-band
recovery procedure documented and tested before going to production.

---

## 4. RPC Subscriber Operational Requirements

This section provides concrete, actionable requirements for any subscriber
polling `synapse-core-contract` events via Stellar RPC.

### 4.1 Normal polling loop

```
Every poll interval (recommended: 1 ledger ≈ 6 s, or at minimum once per minute):

1. Call getEvents with:
   - filters: contractId = <synapse-core-contract-id>, topics = ["synapse", "*"]
   - pagination.cursor = <last_processed_event_id>
     OR startLedger = <last_checkpoint_ledger> if no cursor exists

2. Inspect the response:
   a. Record response.latestLedger and response.latestLedgerCloseTime.
   b. If latestLedger has not advanced since the previous poll AND wall-clock
      time elapsed > 30 s → emit a "RPC node stalled" alert and consider
      switching to a fallback RPC endpoint.

3. Process each event in response.events (ascending id order — this is guaranteed
   by the API):
   a. Deduplicate by event.id (see §4.4).
   b. Dispatch to event handler.
   c. Persist event.id as the new cursor checkpoint (durable write before
      acknowledging the event as processed).

4. Update last_checkpoint_ledger = response.latestLedger for the next poll.
```

**Polling frequency guidance:**

| Use case | Recommended interval |
|---|---|
| Real-time Phase 2/3 trigger (e.g., `done` event starts a swap) | 1 ledger (~6 s) |
| Audit indexer / dashboard | 60 s is fine |
| synapse-web transaction status display | 6–30 s |

Do not poll more frequently than once per ledger — there is nothing new to
observe until the next ledger closes.

### 4.2 Reconnect and gap recovery

When the subscriber restarts or reconnects after any interruption:

```
1. Load last_cursor (event.id) and last_checkpoint_ledger from durable storage.

2. Call getEvents:
   - If last_cursor is set: use pagination.cursor = last_cursor
     (startLedger must be omitted when cursor is used)
   - Else: use startLedger = last_checkpoint_ledger
     (start from the last fully processed ledger to avoid any edge-case
     boundary misses)

3. Check the response for an error:
   - If error code indicates startLedger < oldestLedger → see §4.3.
   - Otherwise, process events as in the normal loop above.

4. After backfilling the gap (response.events is empty or cursor reaches
   latestLedger), resume the normal polling loop.
```

**Important:** The cursor (`event.id`) persisted after step 3c of the normal
loop IS the reconnect recovery point. Losing the cursor means re-scanning from
`last_checkpoint_ledger` instead, which requires processing potentially
duplicate events — handled correctly by the deduplication step in §4.4.

### 4.3 RPC retention window expiry

If the subscriber has been offline long enough that `startLedger < oldestLedger`:

1. **Alert immediately.** This is an operator-action-required condition, not a
   recoverable software path.
2. Fetch the missing ledger range from a historical archive:
   - [Hubble](https://developers.stellar.org/docs/data/analytics/hubble) (SDF
     BigQuery dataset)
   - A full-history Stellar Core node or third-party indexer that retains
     history beyond the RPC window (e.g., [Mercury](https://mercurydata.app),
     [Sierpe](https://sierpe-web.vercel.app/), or other
     [indexer providers](https://developers.stellar.org/docs/data/indexers))
3. Replay the events from the archive into the subscriber's processing pipeline,
   using the same deduplication logic as §4.4 to avoid double-processing events
   the subscriber already handled before going offline.
4. Resume from `oldestLedger` via the normal RPC polling loop.

**Operational requirement:** Every subscriber team MUST have a documented and
tested procedure for this recovery path before going to production. A
subscriber that cannot recover from a 7-day outage is not production-ready.

### 4.4 Deduplication

Stellar RPC documentation explicitly recommends deduplicating events by the
`event.id` field:

> *"If making multiple requests, clients should deduplicate any events received,
> based on the event's unique id field. This prevents double-processing in the
> case of duplicate events being received."*
> — [Stellar Docs: getEvents](https://developers.stellar.org/docs/data/apis/rpc/api-reference/methods/getEvents)

Each `event.id` is a [TOID](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0035.md#specification)-based
identifier: a 19-character monotonically increasing global sequence number
plus a 10-character zero-padded event index, separated by a hyphen
(e.g., `0016010972359577600-0000000001`). TOIDs are globally unique and
ordered by `(ledger_sequence, transaction_index, operation_index, event_index)`.

**Subscriber requirement:**

- Maintain a set of recently seen event IDs (or rely on an idempotency check
  against the downstream action triggered by the event).
- Before processing any event, check: "have I already acted on
  `event.id = X`?". If yes, skip.
- This check must survive process restarts (i.e., persist seen IDs to durable
  storage, or use idempotent downstream writes that produce the same outcome
  if applied twice).

---

## 5. Relationship to Contract Idempotency Guarantees

`synapse-core-contract` provides its own idempotency layer at the registration
level (see `THREAT_MODEL.md §4.2` and `THREAT_MODEL.md R-04`). This is a
separate concern from subscriber-level deduplication:

| Layer | What it protects | Mechanism |
|---|---|---|
| **Contract** (`register_callback`) | Prevents duplicate on-chain writes for the same deposit | `idempotency_key` in temporary storage (~24 h TTL) + persistent `transaction_id` uniqueness guard (no expiry) |
| **Subscriber** (this document) | Prevents double-processing of an event that was already consumed | `event.id` deduplication in subscriber state |

These two layers are complementary. A subscriber that receives the same `done`
event twice (e.g., because of a cursor boundary overlap during reconnect) must
not initiate a second Phase 2 swap. The subscriber's own deduplication — not
the contract's idempotency — is the relevant guard at that point.

The contract's `EventTransactionCompleted` is emitted **at most once per
`tx_id`** by construction (the state machine only allows `Processing →
Completed` once, and it is a terminal state). However, the subscriber may
receive the same event more than once from RPC due to overlapping query ranges
during reconnects. Both sides of the guarantee must hold.

---

## 6. Operational Checklist

Before deploying any event subscriber to production against mainnet, verify:

- [ ] Cursor persistence: the last processed `event.id` is written to durable
      storage (database, Redis with AOF, etc.) before the event's side-effects
      are committed — not after.
- [ ] RPC staleness detection: the subscriber monitors `latestLedger` progress
      and alerts if the RPC node appears to have stalled (no ledger advance in
      > 30 s).
- [ ] Fallback RPC endpoint: a secondary RPC provider is configured and the
      subscriber can switch to it automatically or with a single operator action.
- [ ] Gap recovery procedure: a runbook exists and has been tested for the
      "subscriber offline > 7 days" scenario (§4.3).
- [ ] Deduplication: event.id deduplication is implemented, tested with an
      intentional duplicate replay, and its state persists across restarts.
- [ ] Topic filter: subscriber filters on both `synapse` (topic[0]) and the
      specific event name (topic[1]) as specified in `EVENTS.md §1` — not on
      payload shape alone.
- [ ] `latestLedger` consistency check: after reconnect, the subscriber
      verifies that `latestLedger` in the first response is ≥ the ledger of
      the last processed event. If it is less, the RPC node is behind; do not
      use it as the resumption source until it catches up.
- [ ] No artificial confirmation delay: do not introduce wait-for-N-ledgers
      logic as if Stellar had probabilistic finality. It does not. Artificial
      delays add latency without adding safety.

---

## 7. References

- Stellar Consensus Protocol: <https://developers.stellar.org/docs/learn/fundamentals/stellar-consensus-protocol>
- Stellar RPC `getEvents` method: <https://developers.stellar.org/docs/data/apis/rpc/api-reference/methods/getEvents>
- Stellar RPC event ingestion guide: <https://developers.stellar.org/docs/build/guides/events/ingest>
- Stellar Dashboard (live network metrics): <https://dashboard.stellar.org/>
- Hubble (historical archive): <https://developers.stellar.org/docs/data/analytics/hubble>
- SEP-0035 TOID specification: <https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0035.md>
- synapse-core-contract event catalogue: [`EVENTS.md`](../EVENTS.md)
- Contract threat model (event security §7, idempotency §4.2, R-04): [`THREAT_MODEL.md`](../THREAT_MODEL.md)
- Contract upgrade/admin trust model: [`DECISIONS.md`](../DECISIONS.md)
