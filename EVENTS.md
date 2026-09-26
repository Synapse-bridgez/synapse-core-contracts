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
| [`EventPauseToggled`](#eventpausetoggled) | `pause` | `EventEmitter::pause_toggled` | `pause`, `unpause` | **Live** |
| [`EventContractUpgraded`](#eventcontractupgraded) | `upgrade` | `EventEmitter::contract_upgraded` | `upgrade` | **Live** |
| [`EventStatusChanged`](#eventstatuschanged) | `status` | `EventEmitter::status_changed` | `start_processing`, `complete_transaction`, `fail_transaction`, `expire_transaction` | **Live** |
| [`EventTransactionCompleted`](#eventtransactioncompleted) | `done` | `EventEmitter::transaction_completed` | `complete_transaction` | **Live** |
| [`EventTransactionFailed`](#eventtransactionfailed) | `fail` | `EventEmitter::transaction_failed` | `fail_transaction` | **Live** |
| [`EventTransactionExpired`](#eventtransactionexpired) | `expire` | `EventEmitter::transaction_expired` | `expire_transaction` | **Live** |
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

### EventStatusChanged

| | |
|--|--|
| **Topics** | `synapse`, `status` |
| **Struct** | `EventStatusChanged` |
| **Emitted by** | `start_processing`, `complete_transaction`, `fail_transaction`, `expire_transaction` |
| **When** | Every successful status-machine transition |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | Transaction id |
| `old_status` | `TransactionStatus` | Status before transition |
| `new_status` | `TransactionStatus` | Status after transition |
| `ledger` | `u32` | Ledger sequence at emit |
| `reason` | `Option<String>` | **Additive (v0.1.0).** Optional human-readable reason for the transition. `None` for all pre-existing emitters; populated only by future emitters that need it. |

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
| `settlement_asset` | `Option<String>` | **Additive (v0.1.0).** Optional SEP-11 asset code for the settled leg. `None` for all pre-existing emitters; populated only by future emitters that need it. |

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

### EventTransactionExpired

| | |
|--|--|
| **Topics** | `synapse`, `expire` |
| **Struct** | `EventTransactionExpired` |
| **Emitted by** | `expire_transaction` |
| **When** | A stale `Pending` transaction is auto-expired. Emitted exactly once per successful `expire_transaction` call; never on a rejected/failed attempt. |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | Transaction id |
| `expired_at` | `u32` | Ledger sequence at which the transaction was expired |

**Emission order:** `expire_transaction` emits
[`EventStatusChanged`](#eventstatuschanged) (`Pending` → `Failed`) first, then
`EventTransactionExpired`. Subscribers that only care about the expiry signal
SHOULD filter on the `expire` topic; those tracking the full state machine
SHOULD consume both, in this order.

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
| **When** | Nominee accepts a pending admin transfer, completing the handover |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `old_admin` | `Address` | Admin before the transfer |
| `new_admin` | `Address` | Admin after the transfer |
| `ledger` | `u32` | Ledger sequence at emit |

### EventRelaySignerRotated

| | |
|--|--|
| **Topics** | `synapse`, `relay` |
| **Struct** | `EventRelaySignerRotated` |
| **Emitted by** | `set_relay_signer` |
| **When** | Admin rotates the trusted relay signer |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `old_signer` | `Address` | Previous relay signer |
| `new_signer` | `Address` | New relay signer |
| `ledger` | `u32` | Ledger sequence at emit |

---

## 4. Additive trailing-field pattern (required convention)

Soroban encodes `#[contracttype]` structs as XDR maps keyed by field name, so
adding a field is wire-compatible **only** when the addition is done in a way
that old decoders can still tolerate. To make the semver policy in
[§ Semver policy](#semver-policy) concrete and reusable, all future additive
event changes MUST follow this pattern:

1. **Append, never insert or reorder.** New fields are added at the **end** of
the struct, after every existing field (including `ledger`). Existing field
names, types, and order are frozen.
2. **Wrap the new field in `Option<T>`.** Every additive field MUST be typed
   `Option<T>` so that emitters that do not populate it can pass `None` and old
   subscribers that ignore it can decode the payload without the key.
3. **Emit `None` from all pre-existing call sites.** When the field is added,
   every existing emitter is updated to pass `None`; only new emitters that
   actually have the value populate `Some(..)`. This keeps the change a
   **minor** (or patch) bump, never a major one.
4. **Document the field inline.** Each additive field carries an
   `**Additive (vX.Y.Z).**` note in its catalogue table row describing its
   meaning and the version it was introduced in.
5. **Never remove or retype a field.** Removal, rename, or type change of an
   existing field is a **major** bump and requires a new event name/topic.

### Worked examples

Two high-traffic events already demonstrate the pattern:

- [`EventStatusChanged`](#eventstatuschanged) — trailing `reason: Option<String>`
  appended after `ledger`; all four existing emitters pass `None`.
- [`EventTransactionCompleted`](#eventtransactioncompleted) — trailing
  `settlement_asset: Option<String>` appended after `ledger`; the existing
  emitter passes `None`.

### Subscriber compatibility

A subscriber decoding with the **old** (pre-additive-field) schema against a
**new** payload MUST NOT break. Because the new field is a trailing `Option<T>`,
old decoders that ignore unknown trailing keys continue to work, and new
decoders reading an old payload see the field as `None`. This is verified by the
compatibility test in `src/events.rs`
(`test_additive_field_old_decoder_compatibility`), which decodes a new-shape
payload using old-shape decoding logic and asserts graceful handling.

---

## 5. Semver policy

| Change | Bump |
|--------|------|
| Add a new event (new topic) | minor |
| Add an **additive trailing `Option<T>` field** per [§ 4](#4-additive-trailing-field-pattern-required-convention) | minor or patch |
| Fix a typo in a doc-comment / meaning column | patch |
| Rename a topic, struct, or field | **major** |
| Remove a field | **major** |
| Reorder fields | **major** |
| Change a field’s type | **major** |
| Change multi-event emission order | **major** |

When in doubt, treat the change as **major** and open a discussion in
[`DECISIONS.md`](./DECISIONS.md) before merging.
