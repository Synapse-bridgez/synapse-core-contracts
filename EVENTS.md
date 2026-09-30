# Event Schema — Public API Contract

> **Status:** Locked for Phase 2 / Phase 3 subscribers  
> **Source of truth:** [`src/events.rs`](./src/events.rs)  
> **Conformance gate:** [`event_conformance_manifest.toml`](./event_conformance_manifest.toml) — machine-checked by [`src/test_events_conformance.rs`](./src/test_events_conformance.rs) on every `make check`  
> **Contract version:** `version()` → [`Cargo.toml`](./Cargo.toml) `package.version` (currently **0.1.0**)  
> **Audience:** Swap Engine (Phase 2), Cross-Chain Bridge (Phase 3), off-chain indexers

This document is the **stable public API** for on-chain events emitted by
`synapse-core-contract`. Topic names, data field names/types/order, and
multi-event emission order are part of the contract surface. Changing them is a
**breaking change** for teams that do not share this repo’s release cycle.

Semver policy for this schema lives in [§ Semver policy](#semver-policy) and is
summarised in the README [design-decisions](./README.md#event-schema-as-a-stable-public-api).
Schema revisions are logged separately in [`CHANGELOG.md`](./CHANGELOG.md#event-schema).

> **📖 Subscriber safety reading:**
> Before deploying any event subscriber to production, read
> **[`docs/event-finality.md`](./docs/event-finality.md)** — it covers Stellar's
> BFT finality model (no reorgs, 1-ledger confirmation), how to write a correct
> polling loop, cursor-based reconnect/gap recovery, RPC retention window limits,
> and the relationship between subscriber deduplication and this contract's
> idempotency guarantees.
>
> **synapse-web teams:** review `docs/event-finality.md §4` against your RPC
> poller implementation and flag any discrepancies as issues in this repo.

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
| [`EventTransactionRegistered`](#eventtransactionregistered) | `reg` | `EventEmitter::transaction_registered` | `register_callback` (first write only), `batch_register_callback` | **Live** |
| [`EventBatchProcessed`](#eventbatchprocessed) | `batch` | `EventEmitter::batch_processed` | `batch_register_callback` | **Live** |
| [`EventStatusChanged`](#eventstatuschanged) | `status` | `EventEmitter::status_changed` | every status transition (see §4) | **Live** |
| [`EventTransactionCompleted`](#eventtransactioncompleted) | `done` | `EventEmitter::transaction_completed` | `complete_transaction` | **Live** |
| [`EventTransactionPartiallyCompleted`](#eventtransactionpartiallycompleted) | `partial` | `EventEmitter::transaction_partially_completed` | `partial_complete_transaction` | **Live** |
| [`EventTransactionFailed`](#eventtransactionfailed) | `fail` | `EventEmitter::transaction_failed` | `fail_transaction` | **Live** |
| [`EventTransactionCancelled`](#eventtransactioncancelled) | `cancel` | `EventEmitter::transaction_cancelled` | `cancel_transaction` | **Live** |
| [`EventTransactionRetried`](#eventtransactionretried) | `retry` | `EventEmitter::transaction_retried` | `retry_transaction` | **Live** |
| [`EventTransactionTagged`](#eventtransactiontagged) | `tagged` | `EventEmitter::transaction_tagged` | `add_transaction_tag` | **Live** |
| [`EventTransactionsMerged`](#eventtransactionsmerged) | `merged` | `EventEmitter::transactions_merged` | `merge_duplicate_transactions` | **Live** |
| [`EventForwardingIntent`](#eventforwardingintent) | `fwd` | `EventEmitter::forwarding_intent` | `complete_transaction`, `partial_complete_transaction` (route configured) | **Live** |
| [`EventAmountCeilingSet`](#eventamountceilingset) | `ceiling` | `EventEmitter::amount_ceiling_set` | `set_amount_ceiling`, `set_default_amount_ceiling` | **Live** |
| [`EventAdminTransferProposed`](#eventadmintransferproposed) | `propose` | `EventEmitter::admin_transfer_proposed` | `propose_admin` | **Live** |
| [`EventAdminTransferred`](#eventadmintransferred) | `admin` | `EventEmitter::admin_transferred` | `accept_admin` | **Live** |
| [`EventRelaySignerRotated`](#eventrelaysignerrotated) | `relay` | `EventEmitter::relay_signer_rotated` | `set_relay_signer`, `finalize_relay_signer` | **Live** |
| [`EventRelaySignerChange`](#eventrelaysignerchange) | `rs_prop` | `EventEmitter::relay_signer_proposed` | `propose_relay_signer` | **Live** |
| [`EventRelaySignerChange`](#eventrelaysignerchange) | `rs_canc` | `EventEmitter::relay_signer_change_cancelled` | `cancel_relay_signer_change` | **Live** |
| [`EventRelaySignerMembership`](#eventrelaysignermembership) | `rs_add` | `EventEmitter::relay_signer_added` | `add_relay_signer` | **Live** |
| [`EventRelaySignerMembership`](#eventrelaysignermembership) | `rs_rm` | `EventEmitter::relay_signer_removed` | `remove_relay_signer` | **Live** |
| [`EventRelayThresholdChanged`](#eventrelaythresholdchanged) | `rs_thr` | `EventEmitter::relay_threshold_changed` | `set_relay_threshold` | **Live** |
| [`EventPauseToggled`](#eventpausetoggled) | `pause` | `EventEmitter::pause_toggled` | `pause`, `unpause` | **Live** |
| [`EventContractUpgraded`](#eventcontractupgraded) | `upgrade` | `EventEmitter::contract_upgraded` | `upgrade` / `finalize_upgrade` / `upgrade_and_migrate` / `rollback_upgrade` | **Live** |
| [`EventUpgradeProposed`](#eventupgradeproposed) | `up_prop` | `EventEmitter::upgrade_proposed` | `propose_upgrade` | **Live** |
| [`EventUpgradeFinalized`](#eventupgradefinalized) | `up_fin` | `EventEmitter::upgrade_finalized` | `finalize_upgrade` | **Live** |
| [`EventUpgradeCancelled`](#eventupgradecancelled) | `up_can` | `EventEmitter::upgrade_cancelled` | `cancel_upgrade` | **Live** |
| [`EventUpgradeRolledBack`](#eventupgraderolledback) | `rollback` | `EventEmitter::upgrade_rolled_back` | `rollback_upgrade` | **Live** |
| [`EventUpgradeMigrated`](#eventupgrademigrated) | `migrate` | `EventEmitter::upgrade_migrated` | `upgrade_and_migrate` | **Live** |
| [`EventUpgradeSelfCheckPassed`](#eventupgradeselfcheckpassed) | `chk_pass` | `EventEmitter::upgrade_self_check_passed` | every upgrade path | **Live** |
| [`EventUpgradeSelfCheckFailed`](#eventupgradeselfcheckfailed) | `chk_fail` | `EventEmitter::upgrade_self_check_failed` | every upgrade path (then reverts) | **Live** |
| [`EventParamSet`](#eventparamset) | `param` | `EventEmitter::param_set` | `set_param` | **Live** |
| [`EventBonded`](#eventbonded) | `bonded` | `EventEmitter::bonded` | `bond_collateral` | **Live** |
| [`EventUnbondRequested`](#eventunbondrequested) | `unbondrq` | `EventEmitter::unbond_requested` | `unbond_collateral` | **Live** |
| [`EventUnbondClaimed`](#eventunbondclaimed) | `unbondcl` | `EventEmitter::unbond_claimed` | `claim_unbond` | **Live** |
| [`EventSlashed`](#eventslashed) | `slashed` | `EventEmitter::slashed` | `slash_signer` | **Live** |
| [`EventAnchorTierSet`](#eventanchortierset) | `tierset` | `EventEmitter::anchor_tier_set` | `set_anchor_tier` | **Live** |
| [`EventRebateApplied`](#eventrebateapplied) | `rebate` | `EventEmitter::rebate_applied` | `compute_effective_fee` | **Live** |
| [`EventDisputeRaised`](#eventdisputeraised) | `dispute` | `EventEmitter::dispute_raised` | `dispute_transaction` (sibling issue) | **Schema locked** |
| [`EventDisputeResolved`](#eventdisputeresolved) | `dsprslvd` | `EventEmitter::dispute_resolved` | `resolve_dispute` (sibling issue) | **Schema locked** |

This table is machine-checked: `test_events_conformance::event_decoder_covers_catalogued_topics`
fails if a topic emitted by `src/events.rs` is missing here, or if a topic listed
here is never emitted.

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
| **Emitted by** | `start_processing`, `complete_transaction`, `partial_complete_transaction`, `fail_transaction`, `cancel_transaction`, `retry_transaction`, `merge_duplicate_transactions` |
| **When** | Every successful status-machine transition |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | Transaction id |
| `old_status` | `TransactionStatus` | Status before transition |
| `new_status` | `TransactionStatus` | Status after transition |
| `ledger` | `u32` | Ledger sequence at emit |

`TransactionStatus` variants (discriminant order as in `types.rs`):
`Pending`, `Processing`, `Completed`, `Failed`, `Cancelled`. `Failed` is not
terminal: `retry_transaction` moves it back to `Pending` (at most
`MAX_RETRIES` = 3 times per transaction).

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

### EventTransactionPartiallyCompleted

| | |
|--|--|
| **Topics** | `synapse`, `partial` |
| **Struct** | `EventTransactionPartiallyCompleted` |
| **Emitted by** | `partial_complete_transaction` |
| **When** | Terminal success where only part of the registered amount settled. Replaces `done` for that transition. |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | Transaction id |
| `original_amount` | `i128` | Amount registered by the callback |
| `settled_amount` | `i128` | Amount actually settled (`0 < settled_amount < original_amount`) |
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

### EventTransactionCancelled

| | |
|--|--|
| **Topics** | `synapse`, `cancel` |
| **Struct** | `EventTransactionCancelled` |
| **Emitted by** | `cancel_transaction` |
| **When** | A `Pending` or `Processing` transaction is voided (terminal `Cancelled`) |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | Transaction id |
| `reason` | `String` | Short cancellation code |
| `caller` | `Address` | Relay signer or admin that cancelled |
| `ledger` | `u32` | Ledger sequence at emit |

### EventTransactionRetried

| | |
|--|--|
| **Topics** | `synapse`, `retry` |
| **Struct** | `EventTransactionRetried` |
| **Emitted by** | `retry_transaction` |
| **When** | A `Failed` transaction is moved back to `Pending` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | Transaction id |
| `retry_count` | `u32` | Retries used so far, including this one (max 3) |
| `ledger` | `u32` | Ledger sequence at emit |

### EventTransactionTagged

| | |
|--|--|
| **Topics** | `synapse`, `tagged` |
| **Struct** | `EventTransactionTagged` |
| **Emitted by** | `add_transaction_tag` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | Transaction id |
| `tag` | `String` | Tag added (1–32 bytes) |
| `tag_count` | `u32` | Tags on the transaction after this call (max 8) |
| `ledger` | `u32` | Ledger sequence at emit |

### EventTransactionsMerged

| | |
|--|--|
| **Topics** | `synapse`, `merged` |
| **Struct** | `EventTransactionsMerged` |
| **Emitted by** | `merge_duplicate_transactions` (break-glass admin action) |
| **When** | A duplicate record is linked to its canonical original and moved to `Failed` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `canonical_tx_id` | `String` | Record that stays authoritative |
| `duplicate_tx_id` | `String` | Record marked as merged |
| `admin` | `Address` | Admin that performed the merge |
| `reason` | `String` | Evidence-backed justification |
| `ledger` | `u32` | Ledger sequence at emit |

### EventForwardingIntent

| | |
|--|--|
| **Topics** | `synapse`, `fwd` |
| **Struct** | `EventForwardingIntent` |
| **Emitted by** | `complete_transaction`, `partial_complete_transaction` |
| **When** | Only when the admin configured a route via `set_forwarding_route`; emitted last |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | Completed transaction |
| `next_phase` | `u32` | Phase the transaction should be forwarded to (e.g. `2` = Swap Engine) |
| `ledger` | `u32` | Ledger sequence at emit |

### EventAmountCeilingSet

| | |
|--|--|
| **Topics** | `synapse`, `ceiling` |
| **Struct** | `EventAmountCeilingSet` |
| **Emitted by** | `set_amount_ceiling`, `set_default_amount_ceiling` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `anchor` | `Option<String>` | Anchor (asset issuer) the ceiling applies to; `None` for the contract-wide default |
| `ceiling` | `Option<i128>` | New ceiling; `None` when cleared |
| `admin` | `Address` | Admin that set it |
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
| **Emitted by** | `set_relay_signer`, `finalize_relay_signer` |
| **When** | The primary relay signer changes |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `old_signer` | `Address` | Previous relay signer |
| `new_signer` | `Address` | New relay signer |
| `ledger` | `u32` | Ledger sequence at emit |

### EventRelaySignerChange

| | |
|--|--|
| **Topics** | `synapse`, `rs_prop` / `rs_canc` |
| **Struct** | `EventRelaySignerChange` |
| **Emitted by** | `propose_relay_signer` (`rs_prop`), `cancel_relay_signer_change` (`rs_canc`) |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `new_signer` | `Address` | Proposed primary relay signer |
| `eta_ledger` | `u32` | First ledger `finalize_relay_signer` is legal |
| `ledger` | `u32` | Ledger sequence at emit |

### EventRelaySignerMembership

| | |
|--|--|
| **Topics** | `synapse`, `rs_add` / `rs_rm` |
| **Struct** | `EventRelaySignerMembership` |
| **Emitted by** | `add_relay_signer` (`rs_add`), `remove_relay_signer` (`rs_rm`) |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `signer` | `Address` | Signer added or removed |
| `signer_count` | `u32` | Set size after the change |
| `ledger` | `u32` | Ledger sequence at emit |

### EventRelayThresholdChanged

| | |
|--|--|
| **Topics** | `synapse`, `rs_thr` |
| **Struct** | `EventRelayThresholdChanged` |
| **Emitted by** | `set_relay_threshold` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `old_threshold` | `u32` | Quorum before |
| `new_threshold` | `u32` | Quorum after |
| `ledger` | `u32` | Ledger sequence at emit |

### EventContractUpgraded

| | |
|--|--|
| **Topics** | `synapse`, `upgrade` |
| **Struct** | `EventContractUpgraded` |
| **Emitted by** | `upgrade`, `finalize_upgrade`, `upgrade_and_migrate`, `rollback_upgrade` |
| **When** | After `update_current_contract_wasm` and the post-upgrade self-check succeed |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `admin` | `Address` | Admin that authorised the upgrade |
| `new_wasm_hash` | `BytesN<32>` | New contract wasm hash |
| `ledger` | `u32` | Ledger sequence at emit |
| `schema_version` | `u32` | On-chain schema version the upgrade was checked against |

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

| Field | Type | Meaning |
|-------|------|---------|
| `admin` | `Address` | Admin that finalized |
| `wasm_hash` | `BytesN<32>` | Installed WASM hash |
| `schema_version` | `u32` | On-chain schema version |
| `ledger` | `u32` | Ledger sequence at emit |

### EventUpgradeCancelled

| | |
|--|--|
| **Topics** | `synapse`, `up_can` |
| **Struct** | `EventUpgradeCancelled` |
| **Emitted by** | `cancel_upgrade` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `admin` | `Address` | Admin that cancelled |
| `wasm_hash` | `BytesN<32>` | WASM hash that was pending |
| `ledger` | `u32` | Ledger sequence at emit |

### EventUpgradeRolledBack

| | |
|--|--|
| **Topics** | `synapse`, `rollback` |
| **Struct** | `EventUpgradeRolledBack` |
| **Emitted by** | `rollback_upgrade` |
| **Status** | Live |

Distinct from a forward `upgrade` event so monitors can alert differently.

| Field | Type | Meaning |
|-------|------|---------|
| `admin` | `Address` | Admin that rolled back |
| `restored_wasm_hash` | `BytesN<32>` | Previous WASM hash, now installed again |
| `schema_version` | `u32` | On-chain schema version |
| `ledger` | `u32` | Ledger sequence at emit |

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
| **Emitted by** | every upgrade path |
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
| **Emitted by** | every upgrade path |
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
| `admin` | `Address` | Admin that toggled the pause |
| `ledger` | `u32` | Ledger sequence at emit |

### EventParamSet

| | |
|--|--|
| **Topics** | `synapse`, `param` |
| **Struct** | `EventParamSet` |
| **Emitted by** | `set_param` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `name` | `String` | Parameter name |
| `value` | `i128` | New value |
| `admin` | `Address` | Admin that set it |
| `ledger` | `u32` | Ledger sequence at emit |

### EventBonded

| | |
|--|--|
| **Topics** | `synapse`, `bonded` |
| **Struct** | `EventBonded` |
| **Emitted by** | `bond_collateral` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `signer` | `Address` | Relay signer that bonded |
| `amount` | `i128` | Amount added in this call |
| `total` | `i128` | Bonded total after this call |
| `ledger` | `u32` | Ledger sequence at emit |

### EventUnbondRequested

| | |
|--|--|
| **Topics** | `synapse`, `unbondrq` |
| **Struct** | `EventUnbondRequested` |
| **Emitted by** | `unbond_collateral` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `signer` | `Address` | Relay signer |
| `amount` | `i128` | Amount requested to unbond |
| `claimable_at_ledger` | `u32` | First ledger `claim_unbond` is legal |
| `ledger` | `u32` | Ledger sequence at emit |

### EventUnbondClaimed

| | |
|--|--|
| **Topics** | `synapse`, `unbondcl` |
| **Struct** | `EventUnbondClaimed` |
| **Emitted by** | `claim_unbond` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `signer` | `Address` | Relay signer |
| `amount` | `i128` | Amount released |
| `ledger` | `u32` | Ledger sequence at emit |

### EventSlashed

| | |
|--|--|
| **Topics** | `synapse`, `slashed` |
| **Struct** | `EventSlashed` |
| **Emitted by** | `slash_signer` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `signer` | `Address` | Slashed relay signer |
| `slashed_amount` | `i128` | Amount slashed |
| `remaining_bond` | `i128` | Bond left after slashing |
| `evidence_tx_id` | `String` | `transaction_id` of the conflicting-callback evidence |
| `caller` | `Address` | Admin that submitted the evidence |
| `ledger` | `u32` | Ledger sequence at emit |

### EventAnchorTierSet

| | |
|--|--|
| **Topics** | `synapse`, `tierset` |
| **Struct** | `EventAnchorTierSet` |
| **Emitted by** | `set_anchor_tier` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `anchor` | `Address` | Anchor the tier applies to |
| `rebate_bps` | `u32` | Rebate in basis points (0–10 000) |
| `label` | `String` | Tier label |
| `admin` | `Address` | Admin that set it |
| `ledger` | `u32` | Ledger sequence at emit |

### EventRebateApplied

| | |
|--|--|
| **Topics** | `synapse`, `rebate` |
| **Struct** | `EventRebateApplied` |
| **Emitted by** | `compute_effective_fee` |
| **Status** | Live |

| Field | Type | Meaning |
|-------|------|---------|
| `anchor` | `Address` | Anchor the fee was computed for |
| `base_fee` | `i128` | Fee before rebate |
| `effective_fee` | `i128` | Fee after rebate |
| `rebate_bps` | `u32` | Rebate applied (0 when the anchor has no tier) |
| `ledger` | `u32` | Ledger sequence at emit |

### EventDisputeRaised

| | |
|--|--|
| **Topics** | `synapse`, `dispute` |
| **Struct** | `EventDisputeRaised` |
| **Emitted by** | `dispute_transaction` (sibling issue) |
| **When** | A dispute is opened against a transaction |
| **Status** | Schema locked |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | Transaction under dispute — shared correlation key with `EventDisputeResolved` |
| `reason` | `String` | Short human-readable reason code (e.g. `"amount_mismatch"`, `"missing_settlement"`) |
| `caller` | `Address` | Address that raised the dispute (relay signer or admin) |
| `ledger` | `u32` | Ledger sequence at emit |

**Re-dispute cardinality:** a given `tx_id` may produce more than one
`dispute` / `dsprslvd` event pair over its lifetime if the dispute
state machine permits re-disputing after a prior resolution. Subscribers
MUST correlate pairs by `tx_id` and emission order rather than assuming
at-most-one per transaction. Once the sibling dispute state-machine issue
finalises the cardinality policy, this note will be updated to reflect the
exact rule (once-only or repeatable).

Verified by snapshot-style test `tests::test_dispute_raised_event_payload_snapshot`
(topics `synapse` / `dispute`).

### EventDisputeResolved

| | |
|--|--|
| **Topics** | `synapse`, `dsprslvd` |
| **Struct** | `EventDisputeResolved` |
| **Emitted by** | `resolve_dispute` (sibling issue) |
| **When** | A raised dispute is resolved by the admin |
| **Status** | Schema locked |

| Field | Type | Meaning |
|-------|------|---------|
| `tx_id` | `String` | Transaction ID — shared correlation key with `EventDisputeRaised` |
| `upheld` | `bool` | `true` = dispute upheld (transaction **reverted to `Failed`**); `false` = dispute rejected (transaction **returned to `Completed`**) |
| `caller` | `Address` | Admin address that resolved the dispute |
| `ledger` | `u32` | Ledger sequence at emit |

**`upheld` semantics (normative):**

* `upheld = true` — The dispute is **upheld**. The transaction outcome is
  considered invalid and is **reverted to `Failed`**. Downstream systems
  (support tooling, audit logs) SHOULD treat this as a terminal failure
  equivalent to [`EventTransactionFailed`](#eventtransactionfailed).

* `upheld = false` — The dispute is **rejected**. The original outcome
  stands and the transaction is **returned to `Completed`**. Downstream
  systems SHOULD resume treating the transaction as settled.

Verified by snapshot-style tests
`tests::test_dispute_resolved_upheld_true_event_payload_snapshot` and
`tests::test_dispute_resolved_upheld_false_event_payload_snapshot`
(topics `synapse` / `dsprslvd`).

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
| `complete_transaction` (route configured) | 1. `status`<br>2. `done`<br>3. `fwd` |
| `partial_complete_transaction` | 1. `status` (`Processing` → `Completed`)<br>2. `partial`<br>3. `fwd` *(route configured only)* |
| `fail_transaction` | 1. `status` (`Pending`\|`Processing` → `Failed`)<br>2. `fail` |
| `cancel_transaction` | 1. `status` (`Pending`\|`Processing` → `Cancelled`)<br>2. `cancel` |
| `retry_transaction` | 1. `status` (`Failed` → `Pending`)<br>2. `retry` |
| `batch_register_callback` | 1..N. `reg` (batch order)<br>N+1. `batch` |
| `merge_duplicate_transactions` | 1. `status` (only if the duplicate was not already `Failed`)<br>2. `merged` |
| `propose_admin` | 1. `propose` |
| `accept_admin` | 1. `admin` |
| `set_relay_signer` / `finalize_relay_signer` | 1. `relay` |
| `upgrade` (success) | 1. `chk_pass`<br>2. `upgrade` |
| `upgrade` (self-check failure) | 1. `chk_fail` *(invocation then reverts)* |
| `propose_upgrade` | 1. `up_prop` |
| `finalize_upgrade` | 1. `chk_pass`<br>2. `upgrade`<br>3. `up_fin` |
| `cancel_upgrade` | 1. `up_can` |
| `rollback_upgrade` | 1. `chk_pass`<br>2. `upgrade`<br>3. `rollback` |
| `upgrade_and_migrate` | 1. `chk_pass`<br>2. `upgrade`<br>3. `migrate` |
| `pause` / `unpause` | 1. `pause` |
| `propose_relay_signer` | 1. `rs_prop` |
| `cancel_relay_signer_change` | 1. `rs_canc` |
| `add_relay_signer` / `remove_relay_signer` | 1. `rs_add` / `rs_rm` |
| `set_relay_threshold` | 1. `rs_thr` |

### Worked examples

No event has used the additive pattern yet. `EventContractUpgraded.schema_version`
predates it: it was appended as a plain trailing `u32` before this convention
was written down, and is the only field added after first release.

### Subscriber compatibility

A subscriber decoding with the **old** (pre-additive-field) schema against a
**new** payload MUST NOT break. Because the new field is a trailing `Option<T>`,
old decoders that ignore unknown trailing keys continue to work, and new
decoders reading an old payload see the field as `None`.

Future entry-points wiring the dispute events (sibling issue):

| Entry-point | Order (first → last) |
|-------------|----------------------|
| `dispute_transaction` | 1. `dispute` |
| `resolve_dispute` | 1. `dsprslvd` |

**Rationale for `complete_transaction`:** Phase 2 indexers that listen only to
`done` still see completion; those that key off `status` with
`new_status == Completed` see the transition first, then the hash-bearing
`done` payload. Reordering would break dual-subscriber setups.

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
- Every status transition: `EventStatusChanged` first, then the
  transition-specific event (`done`, `partial`, `fail`, `cancel`, `retry`,
  `merged`) where applicable, then `fwd` if a forwarding route is set.
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

### Automated conformance gate (CI-enforced)

Event-schema conformance is enforced mechanically by two required CI checks
that run as part of `make check` (`cargo test`):

| Test | What it verifies |
|------|-----------------|
| `test_events_conformance::test_events_rs_conforms_to_manifest` | Every struct in `src/events.rs` — field names, types, declaration order, and `symbol_short!` topic — matches `event_conformance_manifest.toml` exactly. |
| `test_events_conformance::test_events_md_conforms_to_manifest` | This file's catalogue tables contain the struct name, topic, and every field name for each event in the manifest. |

The ground-truth manifest is **[`event_conformance_manifest.toml`](./event_conformance_manifest.toml)**
at the repository root. It records the canonical event schema in a
machine-readable form that the conformance tests (`src/test_events_conformance.rs`)
parse and diff against both `src/events.rs` and this document.

**Design rationale:** Soroban's `#[contracttype]` structs compile away
completely in the WASM artefact — there is no runtime type registry to reflect
over. The manifest approach uses `include_str!` source-text parsing (zero new
dependencies) and is documented in the manifest file itself.

**When changing an event:**

1. Update `src/events.rs` (struct fields and/or emitter).
2. Update `event_conformance_manifest.toml` to match.
3. Update the catalogue table(s) in §3 of this file.
4. Run `make check` — all three artefacts must agree or CI blocks.

### Manual review steps

Before merging any PR that touches `src/events.rs` or event emit sites in
`src/lib.rs`:

1. Diff this file against `EventEmitter::*` and the `#[contracttype]` structs.
2. Confirm topic symbols match `symbol_short!(...)` exactly (the §2 table;
   `event_decoder_covers_catalogued_topics` checks this mechanically).
3. Confirm multi-event order in §4 still matches the call sites — the
   conformance tests do not yet check emission order.
4. Run `make check` and confirm all conformance tests pass (automated step above).
5. Run snapshot-style tests (e.g. `test_pause::test_upgrade_emits_contract_upgraded_event`)
   and any new event tests; topics in assertions must match §3.
4. If the schema changed, update [`CHANGELOG.md`](./CHANGELOG.md#event-schema)
   and bump `version()` per §5.

---

## 7. References

- Implementation: [`src/events.rs`](./src/events.rs)
- Status enum: [`src/types.rs`](./src/types.rs) (`TransactionStatus`)
- Upgradability / admin trust: [`DECISIONS.md`](./DECISIONS.md)
- Version probe: `SynapseCoreContract::version`
- **Subscriber finality & reliability guidance:** [`docs/event-finality.md`](./docs/event-finality.md) —
  Stellar BFT finality model, confirmation depth, RPC polling loop, gap recovery,
  retention window, deduplication, and idempotency relationship
