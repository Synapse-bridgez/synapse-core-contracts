# Decision: In-Place Contract Upgradability

> **Status:** Accepted · **Date:** 2025-Q1  
> **Scope:** Phase 1 of the Synapse Bridge ecosystem (`synapse-core-contract`)

---

## 1. Problem Statement

Soroban contracts are **immutable at their deployed WASM hash** by default. Once deployed, the code cannot change unless the contract explicitly opts in via `env.deployer().update_current_contract_wasm()`. This raises a fundamental design question:

> **Should `synapse-core-contract` include an admin-gated upgrade entry point, or remain immutable and rely on a fresh deploy + explicit data-migration whenever changes are needed?**

This decision is time-sensitive because it gets dramatically more expensive once real transactions exist in persistent storage on mainnet.

---

## 2. Options Considered

### Option A — In-Place Upgradability ✅ (Recommended)

Add a single `upgrade(new_wasm_hash: BytesN<32>)` entry point, gated by the existing admin role, that calls `env.deployer().update_current_contract_wasm()`. The admin is already a privileged role (can pause ingestion, rotate relay signer, transfer admin).

**Pros:**
- Bug fixes, event-schema changes, and state-machine extensions can be deployed instantly, without data migration
- Persistent storage (`Admin`, `RelaySigner`, transactions) survives intact — same contract ID, same data
- No need for a "Transaction export/import path" or replay of historical records
- Phase 2 and Phase 3 do not need to re-point to a new contract ID
- Low implementation complexity (~20 lines of production code)

**Cons:**
- Admin key compromise escalates from "can pause/rotate" to "can deploy arbitrary WASM" — a strictly greater trust cost
- Users cannot be guaranteed that the contract they inspected at deployment is the code that will run forever
- Requires strong key-management hygiene (multisig, timelock, or DAO governance — documented as a requirement in README)

**Mitigations:**
1. The admin key **MUST** be held by a multisig (e.g., 3-of-5 Stellar multisig) or a DAO contract, not a single key
2. The admin role already has significant power (pause, rotate signer, transfer admin) — upgrade adds incremental not categorical risk
3. Timelock can be added as a future enhancement (not implemented in Phase 1 but noted as a follow-up)

### Option B — Immutable-with-Migration

Deploy without an upgrade entry point. Any future change requires deploying a new WASM to a new contract ID, exporting all active transactions, and re-pointing the relay + Phase 2/3.

**Pros:**
- Maximum trust-minimisation: the deployed code is the code forever
- No risk of admin-key compromise leading to arbitrary WASM execution
- Users and auditors have cryptographic finality on the code

**Cons:**
- Every bug fix, event change, or state-machine extension requires:
  - Exporting all active (non-terminal) transactions from persistent storage
  - Deploying a new contract
  - Replaying or re-registering exported transactions into the new contract
  - Updating the off-chain relay to point to the new contract ID
  - Updating Phase 2 (Swap Engine) and Phase 3 (Cross-Chain Bridge) to point to the new contract ID
  - Coordinating a network-wide cutover — high operational risk
- Terminal-state (`Completed`/`Failed`) records must still be queryable for audit, requiring either a mirror or archive
- This complexity is especially punishing for a Phase 1 contract that is the foundation for two downstream phases

---

## 3. Recommendation

**Adopt Option A: In-Place Upgradability.**

The recommendation is driven by a pragmatic assessment of the contract's role in the bridge ecosystem:

1. **This is Phase 1 of a multi-phase stack.** Phase 2 and Phase 3 depend on this contract's ID for event subscriptions and queries. Requiring them to re-point on every upgrade introduces systemic fragility that outweighs the marginal trust cost of an admin-gated upgrade.

2. **The admin role is already powerful.** The admin can pause ingestion, rotate the relay signer, and transfer admin. Adding WASM-upgrade capability is incremental within an already-trusted role. The key insight is that the admin **already has the power to disrupt the bridge** — upgrade merely extends the blast radius from operational disruption to code-level modification.

3. **Data-portability risk.** Persistent storage holds transaction records. An immutable contract that needs fixing after mainnet launch would require an export/replay path that introduces its own trust and correctness challenges. In-place upgrades avoid this entirely.

4. **Precedent.** Major Soroban DeFi protocols (e.g., Phoenix, Blend) use admin-gated upgrades. The pattern is well-understood, testable, and auditable.

---

## 4. Upgrade Boundary Guarantees

When the `upgrade()` entry point is invoked, the following guarantees apply:

| Aspect | Behaviour |
|--------|-----------|
| **Contract ID** | Unchanged — all references from Phase 2/3 remain valid |
| **Persistent storage** (admin, relay_signer, transactions) | **Preserved** — data survives intact |
| **Instance storage** (init flag, pause flag) | **Preserved** — survives intact |
| **Temporary storage** (idempotency keys) | **Evicted** — ledger TTL is contract-instance-specific; this is acceptable because idempotency keys are short-lived and the contract will reject duplicates based on existing persistent transaction records |
| **Events** (old contract) | Retained in ledger history — old events are not lost |
| **Trade-off** | The new WASM must be compatible with the existing storage schema (same `StorageKey` enum, same `Transaction` struct layout). A schema-breaking upgrade is a bug, not a feature. |

### When an upgrade is appropriate

- Bug fix in contract logic (e.g., incorrect state-machine guard, validation edge case)
- Adding a new read-only query or event field (extending, not changing, storage)
- State-machine extension for a future phase (e.g., adding a `Refunded` status)

### When a fresh deploy is still required

- Breaking change to `Transaction` struct layout or `StorageKey` variants
- Fundamental shift in access-control model
- The upgrade WASM is too large to fit within Soroban deployment constraints

In these cases, the immutable-with-migration path is still available as a fallback.

---

## 5. Implementation

### Entry point (`lib.rs`)

> **Updated:** the original single-argument `upgrade()` shown when this
> decision was written has since gained an `expected_schema_version` guard
> (THREAT_MODEL.md finding F-04, see [`CHANGELOG.md`](./CHANGELOG.md)). The
> snippet below reflects current code.

```rust
/// Upgrade the contract WASM in-place.
///
/// Only the current admin may call this. The new WASM must be compatible with
/// the existing storage schema. Persistent and instance storage are preserved
/// across the upgrade; temporary storage (idempotency keys) is evicted.
///
/// `expected_schema_version` must match the on-chain schema version or the
/// call is rejected before contract WASM is touched. After the (deferred)
/// WASM swap request, `post_upgrade_self_check` verifies critical storage
/// invariants; failure reverts the whole invocation — including the swap,
/// which the host only commits when the call finishes successfully.
///
/// # Events
/// Emits [`EventUpgradeSelfCheckPassed`] then [`EventContractUpgraded`]
/// on success; [`EventUpgradeSelfCheckFailed`] on self-check failure.
pub fn upgrade(
    env: Env,
    new_wasm_hash: BytesN<32>,
    expected_schema_version: u32,
) -> Result<(), ContractError> {
    let admin = AdminClient::require_admin(&env)?;
    let schema_version = StorageClient::get_schema_version(&env)?;
    if schema_version != expected_schema_version {
        return Err(ContractError::SchemaVersionMismatch);
    }
    let previous_wasm_hash = StorageClient::get_current_wasm_hash(&env);
    env.deployer().update_current_contract_wasm(new_wasm_hash.clone());
    StorageClient::post_upgrade_self_check(&env).map_err(|e| {
        EventEmitter::emit_upgrade_self_check_failed(&env, &e);
        ContractError::SelfCheckFailed
    })?;
    StorageClient::set_current_wasm_hash(&env, &new_wasm_hash);
    EventEmitter::emit_upgrade_self_check_passed(&env);
    EventEmitter::emit_contract_upgraded(&env, &previous_wasm_hash, &new_wasm_hash);
    Ok(())
}
```

---

## 6. Error-code budget

Soroban caps a `#[contracterror]` enum at **50 variants**. The pre-squash
feature set needed **61** conceptually distinct errors, so `ContractError`
(in `src/types.rs`) is a deliberate consolidation: 61 conceptual errors are
mapped onto **≤50 actual variants**. This section is the authoritative record
of every merge decision. A CI assertion (see `scripts/check_error_budget.sh`)
fails the build if the enum ever grows past 50 variants again.

### 6.1 Guiding principle

Every merge preserves the ability for off-chain callers to distinguish the
three operationally meaningful **categories**:

| Category | Meaning | Typical caller response |
|----------|---------|-------------------------|
| **Invalid input** | The caller supplied bad data | Fix the request; do not retry verbatim |
| **Unauthorized** | The caller lacks the required role/signature | Escalate to the right key; do not retry |
| **State-machine forbidden** | The request is well-formed and authorized, but the current state forbids it | Wait / reconcile state; may retry later |

Where two conceptual errors share a variant, they are always in the **same
category**, and the disambiguating detail is carried by the emitted event
(which includes the entry point and, where relevant, the offending values).

### 6.2 Consolidation map (61 → ≤50)

| # | Conceptual error(s) | Variant | Category | Merge rationale |
|---|---------------------|---------|----------|-----------------|
| 1 | `AlreadyInitialized` | `AlreadyInitialized` | State | Kept distinct — idempotent-init callers key on it |
| 2 | `NotInitialized` | `NotInitialized` | State | Kept distinct — every entry point can raise it |
| 3 | `Unauthorized` | `Unauthorized` | Auth | Umbrella for missing-role / wrong-signer |
| 4 | `AdminOnly` | `Unauthorized` | Auth | Same category; event names the entry point |
| 5 | `RelayOnly` | `Unauthorized` | Auth | Same category; event names the entry point |
| 6 | `InvalidSignature` | `InvalidSignature` | Auth | Kept distinct — signature failures are actionable |
| 7 | `InvalidInput` | `InvalidInput` | Input | Umbrella for malformed arguments |
| 8 | `InvalidAmount` | `InvalidInput` | Input | Same category; event carries the amount |
| 9 | `InvalidAddress` | `InvalidInput` | Input | Same category; event carries the address |
| 10 | `InvalidHash` | `InvalidInput` | Input | Same category; event carries the hash |
| 11 | `InvalidSchemaCompatRange` | `InvalidInput` | Input | Same category; event carries the range |
| 12 | `SchemaVersionMismatch` | `SchemaVersionMismatch` | State | Kept distinct — upgrade callers branch on it |
| 13 | `CannotCancel` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 14 | `AlreadyCancelled` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 15 | `InvalidTransition` | `InvalidTransition` | State | Umbrella for all state-machine rejections |
| 16 | `TimelockNotElapsed` | `TimelockNotElapsed` | State | Kept distinct — callers retry after a known delay |
| 17 | `NoPendingChange` | `NoPendingChange` | State | Kept distinct — admin tooling branches on it |
| 18 | `NothingToRollback` | `NoPendingChange` | State | Same category (no pending op); event names the entry point |
| 19 | `UnknownMigration` | `UnknownMigration` | State | Kept distinct — migration tooling branches on it |
| 20 | `MigrationFailed` | `MigrationFailed` | State | Kept distinct — distinct remediation path |
| 21 | `UpgradeNotReversible` | `UpgradeNotReversible` | State | Kept distinct — upgrade tooling branches on it |
| 22 | `SelfCheckFailed` | `SelfCheckFailed` | State | Kept distinct — post-upgrade invariant failure |
| 23 | `RetryLimitExceeded` | `RetryLimitExceeded` | State | Kept distinct — relay backoff logic keys on it |
| 24 | `Paused` | `Paused` | State | Kept distinct — every caller checks it |
| 25 | `NotFound` | `NotFound` | Input | Umbrella for missing records |
| 26 | `TransactionNotFound` | `NotFound` | Input | Same category; event carries the tx id |
| 27 | `DuplicateTransaction` | `DuplicateTransaction` | State | Kept distinct — idempotency callers key on it |
| 28 | `Overflow` | `ArithmeticError` | Input | Same category; event names the operation |
| 29 | `Underflow` | `ArithmeticError` | Input | Same category; event names the operation |
| 30 | `DivisionByZero` | `ArithmeticError` | Input | Same category; event names the operation |
| 31 | `StorageError` | `StorageError` | State | Umbrella for storage-layer failures |
| 32 | `StorageReadFailed` | `StorageError` | State | Same category; event names the key |
| 33 | `StorageWriteFailed` | `StorageError` | State | Same category; event names the key |
| 34 | `SerializationError` | `StorageError` | State | Same category; event names the key |
| 35 | `EventEmitFailed` | `StorageError` | State | Same category; event names the event |
| 36 | `InvalidStatus` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 37 | `InvalidStatusTransition` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 38 | `TerminalState` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 39 | `NotPending` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 40 | `NotCompleted` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 41 | `NotFailed` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 42 | `AlreadyCompleted` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 43 | `AlreadyFailed` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 44 | `AlreadyRefunded` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 45 | `RefundNotAllowed` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 46 | `CancelNotAllowed` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 47 | `RetryNotAllowed` | `InvalidTransition` | State | State-machine forbidden; event carries from/to |
| 48 | `InvalidNonce` | `InvalidInput` | Input | Same category; event carries the nonce |
| 49 | `NonceReused` | `DuplicateTransaction` | State | Same category (replay); event carries the nonce |
| 50 | `InvalidChainId` | `InvalidInput` | Input | Same category; event carries the chain id |
| 51 | `UnsupportedChain` | `InvalidInput` | Input | Same category; event carries the chain id |
| 52 | `InvalidAsset` | `InvalidInput` | Input | Same category; event carries the asset |
| 53 | `UnsupportedAsset` | `InvalidInput` | Input | Same category; event carries the asset |
| 54 | `InvalidFee` | `InvalidInput` | Input | Same category; event carries the fee |
| 55 | `FeeTooHigh` | `InvalidInput` | Input | Same category; event carries the fee |
| 56 | `InvalidDeadline` | `InvalidInput` | Input | Same category; event carries the deadline |
| 57 | `DeadlineExpired` | `TimelockNotElapsed` | State | Same category (time gate); event names the deadline |
| 58 | `InvalidMemo` | `InvalidInput` | Input | Same category; event carries the memo |
| 59 | `MemoTooLong` | `InvalidInput` | Input | Same category; event carries the memo |
| 60 | `InvalidMetadata` | `InvalidInput` | Input | Same category; event carries the metadata |
| 61 | `MetadataTooLarge` | `InvalidInput` | Input | Same category; event carries the size |

### 6.3 Resulting variant set (≤50)

`AlreadyInitialized`, `NotInitialized`, `Unauthorized`, `InvalidSignature`,
`InvalidInput`, `SchemaVersionMismatch`, `InvalidTransition`,
`TimelockNotElapsed`, `NoPendingChange`, `UnknownMigration`,
`MigrationFailed`, `UpgradeNotReversible`, `SelfCheckFailed`,
`RetryLimitExceeded`, `Paused`, `NotFound`, `DuplicateTransaction`,
`ArithmeticError`, `StorageError` — **19 variants**, comfortably under the
50-variant ceiling with headroom for future waves.

### 6.4 Regression guard

`scripts/check_error_budget.sh` counts the variants in the `ContractError`
enum and exits non-zero if the count exceeds 50. It is wired into
`make check` so a future PR cannot silently regress past the cap.
