# Event Schema — Public API Contract

> **Status:** Locked for Phase 2 / Phase 3 subscribers  
> **Source of truth:** [`src/events.rs`](./src/events.rs)  
> **Contract version:** `version()` → [`Cargo.toml`](./Cargo.toml) `package.version` (currently **0.1.0**)  
> **Audience:** Swap Engine (Phase 2), Cross-Chain Bridge (Phase 3), off-chain indexers

This document is the **stable public API** for on-chain events emitted by
`synapse-core-contract`. Topic names, data field names/types/order, and
multi-event emission order are part of the contract surface. Changing them is a
**breaking change** for teams that do not share this repo’s release cycle.

Semver policy for this schema lives in [§ Semver policy](#semver-policy) and is
summarised in the README [design-decisions](./README.md#event-schema-as-a-stable-public-api).
Schema revisions are logged separately in [`CHANGELOG.md`](./CHANGELOG.md#event-schema).

---

## 1. Wire format

Every event is published as:

```text
topics: [ Symbol("synapse"), Symbol("<event_name>") ]
data:   <typed #[contracttype] struct>   // XDR map; field order = Rust declaration order
```

| Index | Topic | Type | Notes |
|------:|-------|------|--------|
| 0 | `synapse` | `Symbol` | Fixed namespace for all Synapse Core events |
| 1 | `<event_name>` | `Symbol` | Short name (`symbol_short!`); see catalogue below |

Subscribers SHOULD filter on both topics. Relying only on payload shape is not
supported.

`ledger` in every payload is `env.ledger().sequence()` at emit time (`u32`).

---

## 2. Implementation status

| Event | Topic[1] | Emitter | Entry-point(s) | Status |
|-------|----------|---------|----------------|--------|
| [`EventInitialised`](#eventinitialised) | `init` | `EventEmitter::initialised` | `initialize` | **Live** |
| [`EventTransactionRegistered`](#eventtransactionregistered) | `reg` | `EventEmitter::transaction_registered` | `register_callback` (first write only) | **Live** |
| [`EventBatchProcessed`](#eventbatchprocessed) | `batch` | `EventEmitter::batch_processed` | `batch_register_callback` | **Live** |
| [`EventPauseToggled`](#eventpausetoggled) | `pause` | `EventEmitter::pause_toggled` | `pause`, `unpause` | **Live** |
| [`EventContractUpgraded`](#eventcontractupgraded) | `upgrade` | `EventEmitter::contract_upgraded` | `upgrade` | **Live** |
| [`EventStatusChanged`](#eventstatuschanged) | `status` | `EventEmitter::status_changed` | `start_processing`, `complete_transaction`, `fail_transaction` | **Live** |
| [`EventTransactionCompleted`](#eventtransactioncompleted) | `done` | `EventEmitter::transaction_completed` | `complete_transaction` | **Live** |
| [`EventTransactionFailed`](#eventtransactionfailed) | `fail` | `EventEmitter::transaction_failed` | `fail_transaction` | **Live** |
| [`EventAdminTransferProposed`](#eventadmintransferproposed) | `propose` | `EventEmitter::admin_transfer_proposed` | `propose_admin` | **Live** |
| [`EventAdminTransferred`](#eventadmintransferred) | `admin` | `EventEmitter::admin_transferred` | `accept_admin` | **Live** |
| [`EventRelaySignerRotated`](#eventrelaysignerrotated) | `relay` | `EventEmitter::relay_signer_rotated` | `set_relay_signer` | **Live** |

**Locked schema** means topics, struct fields, types, and field order are fixed
in this document and in `src/events.rs` even if the `publish` call is still
`todo!()`. Wiring the emitters must match this table exactly — no silent renames.

---

## 3. Event catalogue

Field tables list fields in **declaration / XDR order**. Do not reorder.

### EventInitialised

| | |
|--|--|
| **Topics** | `synapse`, `init` |
| **Struct** | `EventInitialised` |
| **Emitted by** | `initialize` |
| **When** | Once, after admin + relay signer are written |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `admin` | `Address` | Initial admin |
| `relay_signer` | `Address` | Initial trusted relay |
| `ledger` | `u32` | Ledger sequence at emit |

### EventTransactionRegistered

| | |
|--|--|
| **Topics** | `synapse`, `reg` |
| **Struct** | `EventTransactionRegistered` |
| **Emitted by** | `register_callback` |
| **When** | First successful persist of a transaction (**not** on idempotent replay) |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | On-chain / payload transaction id |
| `stellar_account` | `String` | Destination G-address |
| `amount` | `i128` | Amount (stroops / asset units as stored) |
| `asset_code` | `String` | SEP-11 asset code |
| `anchor_transaction_id` | `String` | Anchor Platform id |
| `ledger` | `u32` | Ledger sequence at emit |

### EventBatchProcessed

| | |
|--|--|
| **Topics** | `synapse`, `batch` |
| **Struct** | `EventBatchProcessed` |
| **Emitted by** | `batch_register_callback` |
| **When** | Exactly once per successful batch call, after all per-transaction events for that call |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `caller` | `Address` | Address that invoked the batch entry point |
| `batch_size` | `u32` | Number of transactions in the batch (accurate even when `1`) |
| `first_tx_id` | `String` | `tx_id` of the first item in the batch |
| `last_tx_id` | `String` | `tx_id` of the last item in the batch |
| `ledger` | `u32` | Ledger sequence at emit |

This is a **compact aggregate signal** only. Per-transaction detail (accounts,
amounts, asset codes, anchor ids) is available from the corresponding
[`EventTransactionRegistered`](#eventtransactionregistered) events; subscribers
SHOULD NOT expect the summary to duplicate batch contents.

**Emission ordering (guaranteed):** within a single `batch_register_callback`
call, the per-transaction events are emitted first, in batch order, and the
single `EventBatchProcessed` summary is emitted **last**. A batch of `N`
transactions therefore produces exactly `N` `EventTransactionRegistered` events
followed by exactly one `EventBatchProcessed` event. This holds for the
batch-size-of-one edge case (`N == 1`), where `batch_size == 1` and
`first_tx_id == last_tx_id`.

### EventStatusChanged

| | |
|--|--|
| **Topics** | `synapse`, `status` |
| **Struct** | `EventStatusChanged` |
| **Emitted by** | `start_processing`, `complete_transaction`, `fail_transaction` |
| **When** | Every successful status-machine transition |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | Transaction id |
| `old_status` | `TransactionStatus` | Status before transition |
| `new_status` | `TransactionStatus` | Status after transition |
| `ledger` | `u32` | Ledger sequence at emit |

`TransactionStatus` variants (discriminant order as in `types.rs`):
`Pending`, `Processing`, `Completed`, `Failed`.

Phase 2 / Phase 3 SHOULD treat `new_status == Completed` as the cross-phase
signal (also see [`EventTransactionCompleted`](#eventtransactioncompleted)).

### EventTransactionCompleted

| | |
|--|--|
| **Topics** | `synapse`, `done` |
| **Struct** | `EventTransactionCompleted` |
| **Emitted by** | `complete_transaction` |
| **When** | Terminal success after on-chain settlement is recorded |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | Transaction id |
| `stellar_tx_hash` | `String` | Confirmed Stellar tx hash |
| `ledger` | `u32` | Ledger sequence at emit |

### EventTransactionFailed

| | |
|--|--|
| **Topics** | `synapse`, `fail` |
| **Struct** | `EventTransactionFailed` |
| **Emitted by** | `fail_transaction` |
| **When** | Terminal failure |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | Transaction id |
| `reason` | `String` | Short failure code |
| `ledger` | `u32` | Ledger sequence at emit |

### EventAdminTransferProposed

| | |
|--|--|
| **Topics** | `synapse`, `propose` |
| **Struct** | `EventAdminTransferProposed` |
| **Emitted by** | `propose_admin` |
| **When** | Current admin nominates a new admin. Not yet effective — see [`EventAdminTransferred`](#eventadmintransferred), emitted only once the nominee calls `accept_admin`. |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `current_admin` | `Address` | Admin making the proposal |
| `proposed_admin` | `Address` | Nominated address |
| `ledger` | `u32` | Ledger sequence at emit |

### EventAdminTransferred

| | |
|--|--|
| **Topics** | `synapse`, `admin` |
| **Struct** | `EventAdminTransferred` |
| **Emitted by** | `accept_admin` |
| **When** | Nominee accepts a pending admin transfer, completing it |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `old_admin` | `Address` | Previous admin |
| `new_admin` | `Address` | New admin |
| `ledger` | `u32` | Ledger sequence at emit |

### EventRelaySignerRotated

| | |
|--|--|
| **Topics** | `synapse`, `relay` |
| **Struct** | `EventRelaySignerRotated` |
| **Emitted by** | `set_relay_signer` |
| **When** | Trusted relay signer successfully rotated |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `old_signer` | `Address` | Previous relay signer |
| `new_signer` | `Address` | New relay signer |
| `ledger` | `u32` | Ledger sequence at emit |

### EventContractUpgraded

| | |
|--|--|
| **Topics** | `synapse`, `upgrade` |
| **Struct** | `EventContractUpgraded` |
| **Emitted by** | `upgrade` |
| **When** | After `update_current_contract_wasm` succeeds |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `admin` | `Address` | Admin that authorised the upgrade |
| `new_wasm_hash` | `BytesN<32>` | New contract wasm hash |
| `ledger` | `u32` | Ledger sequence at emit |

### EventPauseToggled

| | |
|--|--|
| **Topics** | `synapse`, `pause` |
| **Struct** | `EventPauseToggled` |
| **Emitted by** | `pause`, `unpause` |
| **When** | Pause state flips |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `paused` | `bool` | New pause state |
| `ledger` | `u32` | Ledger sequence at emit |

---

## 4. Ordering guarantees

Within a single transaction, events are emitted in the order the emitters are
called. The following orderings are part of the contract surface:

- `initialize`: `EventInitialised` only.
- `register_callback`: `EventTransactionRegistered` (first write only).
- `batch_register_callback`: for a batch of `N` items, `N`
  `EventTransactionRegistered` events in batch order, followed by exactly one
  `EventBatchProcessed` summary event. The summary is always last, and is
  emitted even when `N == 1`.
- `start_processing` / `complete_transaction` / `fail_transaction`:
  `EventStatusChanged` first, then the terminal event
  (`EventTransactionCompleted` / `EventTransactionFailed`) where applicable.
- `propose_admin` / `accept_admin` / `set_relay_signer` / `upgrade` / `pause` /
  `unpause`: single event each.

Subscribers that need a per-batch aggregate MUST rely on the single trailing
`EventBatchProcessed` rather than counting per-transaction events, so that
idempotent replays (which do not re-emit `EventTransactionRegistered`) do not
skew the count.

---

## Semver policy

- **Patch** — documentation-only clarifications that do not change topics,
  field names, types, or ordering.
- **Minor** — additive changes: a new event, or a new trailing field on an
  existing struct (subscribers must tolerate unknown trailing fields).
- **Major** — any change to existing topic names, field names, field types,
  field order, or the ordering guarantees in § 4.

Adding `EventBatchProcessed` is a **minor** (additive) change.
