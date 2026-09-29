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
| [`EventContractUpgraded`](#eventcontractupgraded) | `upgrade` | `EventEmitter::contract_upgraded` | `upgrade` / `finalize_upgrade` / `upgrade_and_migrate` / `rollback_upgrade` | **Live** |
| [`EventUpgradeProposed`](#eventupgradeproposed) | `up_prop` | `EventEmitter::upgrade_proposed` | `propose_upgrade` | **Live** |
| [`EventUpgradeFinalized`](#eventupgradefinalized) | `up_fin` | `EventEmitter::upgrade_finalized` | `finalize_upgrade` | **Live** |
| [`EventUpgradeCancelled`](#eventupgradecancelled) | `up_can` | `EventEmitter::upgrade_cancelled` | `cancel_upgrade` | **Live** |
| [`EventUpgradeRolledBack`](#eventupgraderolledback) | `rollback` | `EventEmitter::upgrade_rolled_back` | `rollback_upgrade` | **Live** |
| [`EventUpgradeMigrated`](#eventupgrademigrated) | `migrate` | `EventEmitter::upgrade_migrated` | `upgrade_and_migrate` | **Live** |
| [`EventUpgradeSelfCheckPassed`](#eventupgradeselfcheckpassed) | `chk_pass` | `EventEmitter::upgrade_self_check_passed` | `upgrade` | **Live** |
| [`EventUpgradeSelfCheckFailed`](#eventupgradeselfcheckfailed) | `chk_fail` | `EventEmitter::upgrade_self_check_failed` | `upgrade` | **Live** |
| [`EventStatusChanged`](#eventstatuschanged) | `status` | `EventEmitter::status_changed` | `start_processing`, `complete_transaction`, `fail_transaction`, `expire_transaction` | **Live** |
| [`EventTransactionCompleted`](#eventtransactioncompleted) | `done` | `EventEmitter::transaction_completed` | `complete_transaction` | **Live** |
| [`EventTransactionFailed`](#eventtransactionfailed) | `fail` | `EventEmitter::transaction_failed` | `fail_transaction` | **Live** |
| [`EventTransactionExpired`](#eventtransactionexpired) | `expire` | `EventEmitter::transaction_expired` | `expire_transaction` | **Live** |
| [`EventAdminTransferProposed`](#eventadmintransferproposed) | `propose` | `EventEmitter::admin_transfer_proposed` | `propose_admin` | **Live** |
| [`EventAdminTransferred`](#eventadmintransferred) | `admin` | `EventEmitter::admin_transferred` | `accept_admin` | **Live** |
| [`EventRelaySignerRotated`](#eventrelaysignerrotated) | `relay` | `EventEmitter::relay_signer_rotated` | `set_relay_signer` | **Live** |
| [`EventUpgradeQuorumSet`](#eventupgradequorumset) | `uqset` | `EventEmitter::upgrade_quorum_set` | `set_upgrade_quorum` | **Live** |
| [`EventUpgradeProposed`](#eventupgradeproposed) | `uprop` | `EventEmitter::upgrade_proposed` | `propose_upgrade` | **Live** |
| [`EventSignerAttestationSet`](#eventsignerattestationset) | `attest` | `EventEmitter::signer_attestation_set` | `set_signer_attestation` | **Live** |
| [`EventAdminRenounced`](#eventadminrenounced) | `renounce` | `EventEmitter::admin_renounced` | `renounce_admin` | **Live** |
| [`EventGuardianSet`](#eventguardianset) | `guardian` | `EventEmitter::guardian_set` | `set_guardian` | **Live** |
| [`EventAutoPaused`](#eventautopaused) | `apause` | `EventEmitter::auto_paused` | `trip_auto_pause` | **Live** |
| [`EventAutoUnpaused`](#eventautounpaused) | `aunpause` | `EventEmitter::auto_unpaused` | `unpause_auto` (quorum met) | **Live** |

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

### EventUpgradeQuorumSet

| | |
|--|--|
| **Topics** | `synapse`, `uqset` |
| **Struct** | `EventUpgradeQuorumSet` |
| **Emitted by** | `set_upgrade_quorum` |
| **When** | Optional upgrade M-of-N quorum is configured or cleared (#87) |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `admin` | `Address` | Admin that wrote the config |
| `threshold` | `u32` | M (0 when cleared) |
| `member_count` | `u32` | N (0 when cleared) |
| `ledger` | `u32` | Ledger sequence at emit |

### EventUpgradeProposed

| | |
|--|--|
| **Topics** | `synapse`, `uprop` |
| **Struct** | `EventUpgradeProposed` |
| **Emitted by** | `propose_upgrade` |
| **When** | Upgrade staged awaiting quorum co-signatures (#87) |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `proposer` | `Address` | Admin that proposed |
| `new_wasm_hash` | `BytesN<32>` | Proposed WASM hash |
| `expected_schema_version` | `u32` | Schema version checked at propose time |
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

### EventUpgradeProposed

| | |
|--|--|
| **Topics** | `synapse`, `up_prop` |
| **Struct** | `EventUpgradeProposed` |
| **Emitted by** | `propose_upgrade` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `admin` | `Address` | Admin that proposed |
| `wasm_hash` | `BytesN<32>` | Pending WASM hash |
| `expected_schema_version` | `u32` | Schema arg checked at propose + finalize |
| `eta_ledger` | `u32` | First ledger finalize is legal |
| `ledger` | `u32` | Ledger sequence at emit |

### EventUpgradeFinalized

| | |
|--|--|
| **Topics** | `synapse`, `up_fin` |
| **Struct** | `EventUpgradeFinalized` |
| **Emitted by** | `finalize_upgrade` |
| **Status** | Live |

Emitted in addition to `EventContractUpgraded` after a successful timelocked swap.

### EventUpgradeCancelled

| | |
|--|--|
| **Topics** | `synapse`, `up_can` |
| **Struct** | `EventUpgradeCancelled` |
| **Emitted by** | `cancel_upgrade` |
| **Status** | Live |

### EventUpgradeRolledBack

| | |
|--|--|
| **Topics** | `synapse`, `rollback` |
| **Struct** | `EventUpgradeRolledBack` |
| **Emitted by** | `rollback_upgrade` |
| **Status** | Live |

Distinct from a forward `upgrade` event so monitors can alert differently.

### EventUpgradeMigrated

| | |
|--|--|
| **Topics** | `synapse`, `migrate` |
| **Struct** | `EventUpgradeMigrated` |
| **Emitted by** | `upgrade_and_migrate` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `admin` | `Address` | Admin that authorised |
| `migration_id` | `u32` | Registry id that ran |
| `storage_touches` | `u32` | Touches reported by the routine |
| `new_wasm_hash` | `BytesN<32>` | Installed WASM |
| `ledger` | `u32` | Ledger sequence at emit |

### EventUpgradeSelfCheckPassed

| | |
|--|--|
| **Topics** | `synapse`, `chk_pass` |
| **Struct** | `EventUpgradeSelfCheckPassed` |
| **Emitted by** | `upgrade` |
| **When** | Post-upgrade storage-integrity self-check succeeded |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `schema_version` | `u32` | On-chain schema version verified by the self-check |
| `ledger` | `u32` | Ledger sequence at emit |

### EventUpgradeSelfCheckFailed

| | |
|--|--|
| **Topics** | `synapse`, `chk_fail` |
| **Struct** | `EventUpgradeSelfCheckFailed` |
| **Emitted by** | `upgrade` |
| **When** | Post-upgrade self-check failed; the upgrade transaction reverts |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `schema_version` | `u32` | On-chain schema version at the time of the failed check |
| `ledger` | `u32` | Ledger sequence at emit |

Verified by `test_pause::test_self_check_events_topics`.

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

### EventSignerAttestationSet

| | |
|--|--|
| **Topics** | `synapse`, `attest` |
| **Struct** | `EventSignerAttestationSet` |
| **Emitted by** | `set_signer_attestation` |
| **When** | Every attestation write, including no-op same-hash updates |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `signer` | `Address` | Relay signer that self-reported the fingerprint |
| `build_hash` | `BytesN<32>` | Self-reported build/commit fingerprint (not cryptographically verified on-chain) |
| `ledger` | `u32` | Ledger sequence at emit |

### EventAdminRenounced

| | |
|--|--|
| **Topics** | `synapse`, `renounce` |
| **Struct** | `EventAdminRenounced` |
| **Emitted by** | `renounce_admin` |
| **When** | Outgoing admin acknowledges step-down after a live successor accepted |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `former_admin` | `Address` | Outgoing admin |
| `successor` | `Address` | Live admin installed by prior `accept_admin` |
| `ledger` | `u32` | Ledger sequence at emit |

### EventGuardianSet

| | |
|--|--|
| **Topics** | `synapse`, `guardian` |
| **Struct** | `EventGuardianSet` |
| **Emitted by** | `set_guardian` |
| **When** | Guardian address set or rotated |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `guardian` | `Address` | New guardian |
| `ledger` | `u32` | Ledger sequence at emit |

### EventAutoPaused

| | |
|--|--|
| **Topics** | `synapse`, `apause` |
| **Struct** | `EventAutoPaused` |
| **Emitted by** | `trip_auto_pause` |
| **When** | Automatic circuit-breaker pause engaged |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `ledger` | `u32` | Ledger sequence at emit |

### EventAutoUnpaused

| | |
|--|--|
| **Topics** | `synapse`, `aunpause` |
| **Struct** | `EventAutoUnpaused` |
| **Emitted by** | `unpause_auto` (when 2-of-3 quorum is met) |
| **When** | Automatic pause released by a role pair |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `roles` | `UnpauseRoles` | Winning pair: `AdminGuardian` / `AdminRelay` / `GuardianRelay` |
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

| Entry-point | Order (first → last) |
|-------------|----------------------|
| `initialize` | 1. `init` |
| `register_callback` (first write) | 1. `reg` |
| `register_callback` (idempotent hit) | *(no events)* |
| `start_processing` | 1. `status` (`Pending` → `Processing`) |
| `complete_transaction` | 1. `status` (`Processing` → `Completed`)<br>2. `done` |
| `fail_transaction` | 1. `status` (`Pending`\|`Processing` → `Failed`)<br>2. `fail` |
| `propose_admin` | 1. `propose` |
| `accept_admin` | 1. `admin` |
| `set_relay_signer` | 1. `relay` |
| `upgrade` (success) | 1. `chk_pass`<br>2. `upgrade` |
| `upgrade` (self-check failure) | 1. `chk_fail` *(invocation then reverts)* |
| `propose_upgrade` | 1. `up_prop` |
| `finalize_upgrade` | 1. `upgrade`<br>2. `up_fin` |
| `cancel_upgrade` | 1. `up_can` |
| `rollback_upgrade` | 1. `upgrade`<br>2. `rollback` |
| `upgrade_and_migrate` | 1. `upgrade`<br>2. `migrate` |
| `pause` / `unpause` | 1. `pause` |
| `set_signer_attestation` | 1. `attest` |
| `renounce_admin` | 1. `renounce` |
| `set_guardian` | 1. `guardian` |
| `trip_auto_pause` | 1. `apause` |
| `unpause_auto` (quorum met) | 1. `aunpause` |
| `unpause_auto` (vote only) | *(no events)* |

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

## 5. Ordering guarantees

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

| Change | Version bump | Advance notice |
|--------|--------------|----------------|
| Add a **new optional field at the end** of an existing event struct\* | **Minor** or **Patch** | Recommended |
| Add a **new event** (new topic[1] + struct) | **Minor** | Recommended |
| Document-only / non-behavioural clarifications | **Patch** | Not required |
| **Remove** a field | **Major** | **Required** — notify Phase 2 / Phase 3 |
| **Rename** a field or topic symbol | **Major** | **Required** |
| **Reorder** fields in a `#[contracttype]` struct | **Major** | **Required** |
| **Change** a field’s type | **Major** | **Required** |
| Change which events fire on a transition, or **emission order** | **Major** | **Required** |
| Change topic[0] away from `synapse` | **Major** | **Required** |

\*Soroban `#[contracttype]` structs are positional in XDR. “Additive at the end”
is the only additive pattern allowed without a major bump; inserting a field in
the middle is a **Major** (reorder). Prefer a **new event** over mid-struct
inserts when in doubt.

Adding `EventBatchProcessed` is a **minor** (additive) change.

When in doubt, treat the change as **major** and open a discussion in
[`DECISIONS.md`](./DECISIONS.md) before merging.

### Advance notice

For any **Major** event-schema change:

1. Open / update an issue tagged for subscriber teams **before** merging.
2. Record the planned break under [`CHANGELOG.md` → Event schema → Unreleased](./CHANGELOG.md#event-schema).
3. Bump `version()` major in the same release that ships the break.
4. Keep the old behaviour available until the noticed cutover date when
   operationally possible (dual-emit is allowed only within a documented
   migration window and itself requires changelog entries).

---

## 6. Verification checklist (maintainers)

Before merging any PR that touches `src/events.rs` or event emit sites in
`src/lib.rs`:

1. Diff this file against `EventEmitter::*` and the `#[contracttype]` structs.
2. Confirm topic symbols match `symbol_short!(...)` exactly (`init`, `reg`,
   `pause`, `upgrade`, `status`, `done`, `fail`, `admin`, `relay`, `propose`,
   `up_prop`, `up_fin`, `up_can`, `rollback`, `migrate`,
   `attest`, `renounce`, `guardian`, `apause`, `aunpause`).
3. Confirm multi-event order in §4 still matches the call sites.
4. Run snapshot-style tests (e.g. `test_pause::test_upgrade_emits_contract_upgraded_event`)
   and any new event tests; topics in assertions must match §3.
5. If the schema changed, update [`CHANGELOG.md`](./CHANGELOG.md#event-schema)
   and bump `version()` per §5.

---

## 7. References

- Implementation: [`src/events.rs`](./src/events.rs)
- Status enum: [`src/types.rs`](./src/types.rs) (`TransactionStatus`)
- Upgradability / admin trust: [`DECISIONS.md`](./DECISIONS.md)
- Version probe: `SynapseCoreContract::version`

## Addendum: `merged` event

`EventTransactionsMerged { canonical_tx_id, duplicate_tx_id, admin, reason, ledger }`,
topic `merged`, emitted once by `merge_duplicate_transactions` (break-glass admin action).
