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

```rust
/// Upgrade the contract WASM in-place.
///
/// Only the current admin may call this. The new WASM must be compatible with
/// the existing storage schema. Persistent and instance storage are preserved
/// across the upgrade; temporary storage (idempotency keys) is evicted.
///
/// # Events
/// Emits [`EventContractUpgraded`] on success.
pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) -> Result<(), ContractError> {
    let admin = AdminClient::require_admin(&env)?;
    env.deployer().update_current_contract_wasm(new_wasm_hash);
    EventEmitter::contract_upgraded(&env, &admin, &new_wasm_hash);
    Ok(())
}
```

### New event (`events.rs`)

```rust
#[contracttype]
pub struct EventContractUpgraded {
    pub admin: soroban_sdk::Address,
    pub new_wasm_hash: soroban_sdk::BytesN<32>,
    pub ledger: u32,
}
```

---

## 6. Trust Assumptions (Documented in README)

The admin key is the **single most important secret** in the Synapse Bridge ecosystem. Compromise of the admin key allows:

1. Contract upgrade to arbitrary WASM (code execution)
2. Pause/unpause of callback ingestion (denial of service)
3. Rotation of admin role (permanent loss of control)
4. Rotation of relay signer (theft via fake callbacks)

**Mandatory requirements:**
- The admin key MUST be held by a **multisig** (minimum 3-of-5) or a **timelock-governed DAO**
- Admin operations SHOULD be logged and monitored off-chain
- The admin key SHOULD NOT be the same key used for deployment or relay signing

---

## 7. Future Enhancements (Out of Scope for Phase 1)

- **Timelocked upgrade**: Require a two-step process where `schedule_upgrade()` sets a pending hash and `execute_upgrade()` can only be called after N ledgers
- **Emergency pause before upgrade**: Require the contract to be paused before an upgrade can execute, preventing race conditions with in-flight callbacks
- **DAO-controlled admin**: Replace the single-address admin with a Soroban DAO contract

---

## 8. References

- [Soroban Docs: Contract Upgrades](https://developers.stellar.org/docs/smart-contracts/getting-started/upgrading)
- [SEP-11: Stellar Asset Code Conventions](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0011.md)
- [Phoenix Protocol: Upgradeable Pools](https://github.com/Phoenix-Protocol-Group/phoenix)

