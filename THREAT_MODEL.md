# Threat Model & Security-Audit Readiness Report

> **Status:** Draft — pre-audit self-review  
> **Date:** 2026-07-21  
> **Scope:** `synapse-core-contract` — Phase 1 of the Synapse Bridge ecosystem  
> **Contract version:** see `version()` entry point (current: 0.1.0)  
> **Authors:** Synapse Bridge core team  
> **Gating item for:** mainnet deployment

This document is the gating deliverable before a mainnet deployment is
considered. It covers the actor model, per-entry-point attack surfaces,
state-machine invariants, accepted risks with compensating controls, and the
pre-audit checklist an external auditor will require.

---

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Actor Model](#2-actor-model)
3. [Trust Boundaries](#3-trust-boundaries)
4. [Attack Surfaces — Entry Points](#4-attack-surfaces--entry-points)
5. [State-Machine Invariants](#5-state-machine-invariants)
6. [Storage Security](#6-storage-security)
7. [Event Security](#7-event-security)
8. [Accepted Risks & Compensating Controls](#8-accepted-risks--compensating-controls)
9. [Self-Review Findings](#9-self-review-findings)
10. [External Auditor Checklist](#10-external-auditor-checklist)

---

## 1. System Overview

`synapse-core-contract` is the on-chain component of the Synapse Bridge Phase 1
fiat gateway. It sits at the **trust boundary** between the off-chain relay
service (`synapse-core`) and the rest of the Stellar ecosystem.

```
Internet / Anchor Platform
          │
          │  HTTPS webhook (off-chain)
          ▼
┌─────────────────────┐
│  synapse-core        │   Off-chain relay service
│  (relay_signer key)  │   Validates, deduplicates, forwards callbacks
└─────────┬───────────┘
          │  Stellar transaction (signed by relay_signer)
          ▼
┌────────────────────────────────────────────────────┐
│            synapse-core-contract                   │  ◄── this document
│  ┌──────────┐  ┌──────────┐  ┌──────────────────┐ │
│  │  Storage │  │  Events  │  │  Access Control  │ │
│  │ (ledger) │  │ (horizon)│  │ (admin/relay)    │ │
│  └──────────┘  └──────────┘  └──────────────────┘ │
└────────────────────────────────────────────────────┘
          │  Events
          ▼
Phase 2 (Swap Engine) / Phase 3 (Cross-Chain Bridge)
```

### What the contract does

1. Accepts callback registrations from the off-chain relay, storing each
   deposit event with status `Pending`.
2. Guards against duplicate delivery with a temporary-storage idempotency key.
3. Drives the transaction lifecycle: `Pending → Processing → Completed | Failed`.
4. Emits structured events at every state transition for downstream subscribers.
5. Provides an admin-gated emergency pause (circuit breaker).
6. Supports in-place WASM upgrade via an admin-gated `upgrade()` entry point.

---

## 2. Actor Model

| Actor | Identity | Trust Level | Capabilities |
|-------|----------|-------------|--------------|
| **Admin** | Stellar address stored in `StorageKey::Admin` (persistent). Must be a ≥3-of-5 multisig or DAO. | **High — semi-trusted** | `pause`, `unpause`, `upgrade`, `transfer_admin`, `set_relay_signer`; also permitted to call status-transition methods. |
| **Relay signer** | Stellar address stored in `StorageKey::RelaySigner` (persistent). Key held by the off-chain `synapse-core` service. | **High — semi-trusted** | `register_callback`; also permitted to call status-transition methods (`start_processing`, `complete_transaction`, `fail_transaction`). |
| **Unauthenticated caller** | Any Stellar account that submits a transaction to this contract. | **Untrusted** | Read-only queries only: `get_transaction`, `get_status`, `is_duplicate`, `is_paused`, `health`, `version`. |
| **Deployer** | The account that ran `stellar contract deploy`. Distinct from the admin key (see `DEPLOYMENT.md`). | **One-time, then irrelevant** | None after `initialize()` is called. |
| **Phase 2 / Phase 3** | Downstream off-chain or on-chain services that subscribe to events. | **Consumers only** | Read events from Horizon/RPC. Cannot mutate contract state. |

### Actor threat profiles

**Admin (compromised):**  
The admin key is the highest-value target. Compromise allows: arbitrary WASM
deployment via `upgrade()`, forced pause/unpause disrupting ingestion, relay
signer rotation enabling fake callback injection, and admin role transfer
(permanent loss of control). The admin **must** be a multisig.

**Relay signer (compromised):**  
Compromise allows injection of fraudulent `register_callback` calls with
attacker-controlled `stellar_account`, `amount`, and asset fields. The relay
signer can also drive status transitions, potentially completing or failing
transactions it did not legitimately register. If the relay key is a hot key
(single-sig, held by the running service), key exfiltration via a server
compromise is the primary threat vector.

**Unauthenticated caller:**  
No write access. The primary risk is resource exhaustion via high-volume
read queries (CPU/bandwidth on the node, not ledger state). Soroban's
per-invocation fee metering mitigates this.

---

## 3. Trust Boundaries

```
┌─────────────────────────────────────────────────────────────────┐
│  UNTRUSTED ZONE                                                 │
│  Any Stellar account; arbitrary callers                         │
│                    │                                            │
│         read-only queries only                                  │
└────────────────────┼────────────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────────────┐
│  SEMI-TRUSTED ZONE A: relay_signer                              │
│  Off-chain hot key; single point of relay trust in Phase 1      │
│                    │                                            │
│     register_callback, start_processing,                        │
│     complete_transaction, fail_transaction                      │
└────────────────────┼────────────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────────────┐
│  SEMI-TRUSTED ZONE B: admin (must be multisig / DAO)            │
│  All relay_signer capabilities PLUS:                            │
│  pause, unpause, upgrade, transfer_admin, set_relay_signer      │
└─────────────────────────────────────────────────────────────────┘
```

**Key boundary observations:**

1. The contract has **no on-chain oracle** — it cannot independently verify
   that the Anchor Platform webhook was genuine. It fully trusts the relay
   signer to have performed that verification off-chain.

2. There is **no second-factor check** on the `stellar_account` field beyond
   format validation (56-char G-address). The contract cannot verify that the
   depositing account actually sent funds on Horizon; that verification is the
   relay's responsibility.

3. The `stellar_tx_hash` recorded in `complete_transaction` is supplied by the
   relay. The contract does **not** independently verify it against Horizon.

---

## 4. Attack Surfaces — Entry Points

### 4.1 `initialize(admin, relay_signer)`

| Threat | Mitigation | Status |
|--------|-----------|--------|
| Double-initialisation to hijack admin/relay roles | `StorageClient::is_initialised()` guard; returns `AlreadyInitialised` on second call | ✅ Implemented |
| Front-running: attacker calls `initialize()` before legitimate deployer | Deployer must call `initialize()` atomically in the deployment transaction, or use a deployer script that deploys + initialises in one transaction | ⚠️ Operational control — not enforced on-chain |
| Admin set to zero address or uncontrolled account | No on-chain check that `admin` is a multisig; documented requirement only | ⚠️ Operational control — not enforced on-chain |
| Relay signer set to same address as admin | No on-chain separation check | ⚠️ Low risk — separation is a best-practice recommendation |

### 4.2 `register_callback(payload)`

| Threat | Mitigation | Status |
|--------|-----------|--------|
| Unauthenticated callback injection | `relay.require_auth()` enforced before any write | ✅ Implemented |
| Replay / duplicate delivery | Idempotency key checked in temporary storage; duplicate returns original `tx_id` | ✅ Implemented |
| Idempotency TTL expiry enabling re-registration of the same key | Idempotency TTL is ~24 h; after expiry, the same key could re-register. The contract will still reject it if a `Transaction` record already exists with the same `transaction_id` (TransactionNotFound check is implicit on duplicate payload). **Note: this second guard relies on `transaction_id` uniqueness being enforced off-chain.** | ⚠️ Partial — relies on off-chain UUID uniqueness |
| Oversized string fields causing unbounded storage rent | `validation.rs` caps: `tx_id` ≤ 64 B, `anchor_tx_id` ≤ 64 B, `callback_status` ≤ 32 B | ✅ Implemented |
| Malformed `stellar_account` / `asset_issuer` (non-G address) | Length == 56 and first byte == `G` check in `Validator::validate_stellar_account` | ✅ Implemented — **see Finding F-01** |
| Zero or negative `amount` | `validate_amount` rejects `amount <= 0` | ✅ Implemented |
| Empty `asset_code` or non-uppercase chars | `validate_asset_code` enforces 1–12 uppercase ASCII | ✅ Implemented |
| Empty `idempotency_key` | `validate_idempotency_key` rejects empty | ✅ Implemented |
| Callback ingestion while paused | Pause check is the **first** guard in the function body | ✅ Implemented |

### 4.3 `start_processing(tx_id, caller)` / `complete_transaction(tx_id, stellar_tx_hash, caller)` / `fail_transaction(tx_id, reason, caller)`

> **Note:** These three entry points are **not yet implemented** (bodies are `todo!()`).  
> The threats below represent requirements the implementation MUST satisfy.

| Threat | Required Mitigation | Status |
|--------|---------------------|--------|
| Unauthenticated state transition | `caller.require_auth()` + `AdminClient::assert_is_relay_or_admin()` | ❌ Not yet implemented |
| Invalid state transition (e.g., skip Pending → directly to Completed) | Guard: assert `tx.status == expected_prior_status` before writing | ❌ Not yet implemented |
| Transition of non-existent transaction | `StorageClient::get_transaction()` returns `TransactionNotFound`; must propagate | ❌ Not yet implemented |
| `stellar_tx_hash` not validated in `complete_transaction` | `Validator::validate_stellar_tx_hash()` exists (max 72 B) — must be called | ❌ Not yet implemented |
| `reason` not validated in `fail_transaction` | `Validator::validate_failure_reason()` exists (max 64 B) — must be called | ❌ Not yet implemented |
| Re-completing or re-failing a terminal transaction | Guard: `Completed` and `Failed` are terminal — must reject any transition from these states | ❌ Not yet implemented |

---

### 4.4 `transfer_admin(new_admin)` / `set_relay_signer(new_signer)`

> **Note:** Both entry points are **not yet implemented** (bodies are `todo!()`).

| Threat | Required Mitigation | Status |
|--------|---------------------|--------|
| Non-admin rotating admin or relay | `AdminClient::require_admin()` must be called first | ❌ Not yet implemented |
| Admin transferring to an attacker-controlled address (key-compromise scenario) | No on-chain prevention; mitigated by multisig — any single-key compromise cannot unilaterally rotate | ⚠️ Operational control |
| Admin transferred to zero/null address (bricking the contract) | No on-chain guard; recommend adding a check that `new_admin` is not the zero address | 🔲 Open finding — see **F-02** |
| No two-step confirmation for admin transfer | Single-step transfer accepted in Phase 1; two-step pattern is a recommended enhancement | ⚠️ Accepted risk — see **F-03** |
| `set_relay_signer` does not emit an event | `EventEmitter::admin_transferred` pattern must be extended for relay rotation | ❌ Not yet implemented |

### 4.5 `upgrade(new_wasm_hash)`

| Threat | Mitigation | Status |
|--------|-----------|--------|
| Non-admin deploying arbitrary WASM | `AdminClient::require_admin()` + `require_auth()` enforced before `update_current_contract_wasm()` | ✅ Implemented |
| Storage-schema-breaking upgrade corrupting persistent records | Documented requirement only — no on-chain schema version check | ⚠️ Operational control — see **F-04** |
| Upgrade without pausing, racing with in-flight callbacks | No forced-pause pre-condition on upgrade; documented as future enhancement in `DECISIONS.md` | ⚠️ Accepted risk — see **F-05** |
| Upgrade event not emitted, hiding the action | `EventEmitter::contract_upgraded()` always called on success | ✅ Implemented |
| Admin upgrades to WASM that removes the `upgrade()` entry point (bricking upgradability) | No on-chain prevention; admin is the sole trust anchor | ⚠️ Operational control |

### 4.6 `pause()` / `unpause()`

| Threat | Mitigation | Status |
|--------|-----------|--------|
| Non-admin toggling pause | `AdminClient::require_admin()` enforced | ✅ Implemented |
| Pause not being idempotent (reverts if already paused) | `set_paused(true/false)` is a simple store — idempotent by construction | ✅ Implemented |
| Status transitions proceeding while paused (draining is intentional) | `start_processing`, `complete_transaction`, `fail_transaction` are deliberately not gated by pause | ✅ By design — documented in `lib.rs` |
| Attacker forcing a DoS via pause (requires admin-key compromise) | Mitigated by multisig admin requirement | ⚠️ Residual risk — requires admin compromise |

### 4.7 Read-Only Queries (`get_transaction`, `get_status`, `is_duplicate`, `is_paused`, `health`, `version`)

| Threat | Mitigation | Status |
|--------|-----------|--------|
| Unauthenticated reads leaking sensitive data | Transaction data is public on-chain by nature; no PII expected in stored fields | ✅ Acceptable — Stellar ledger is public |
| `get_transaction` and `get_status` not yet fully implemented (`get_status`, `is_duplicate` are `todo!()`) | Tracked as implementation gap — see **F-06** | ❌ Not yet implemented |
| TTL extension in `get_transaction` allowing an attacker to indefinitely extend storage rent on a record | TTL extension is bounded and is the intended behaviour for active records; not exploitable beyond keeping legitimate data alive | ✅ Acceptable |

---

## 5. State-Machine Invariants

The following invariants must hold at all times. Each must be verified by the
implementation and covered by tests before the contract is considered audit-ready.

```
Pending ──► Processing ──► Completed
        └──────────────► Failed
```

| # | Invariant | Enforced By | Status |
|---|-----------|-------------|--------|
| I-01 | A transaction can only be created in `Pending` state | `register_callback` always sets `status: TransactionStatus::Pending` | ✅ Implemented |
| I-02 | `Pending → Processing` is the only valid transition for `start_processing` | Guard: `tx.status == Pending` | ❌ Not yet implemented |
| I-03 | `Processing → Completed` is the only valid transition for `complete_transaction` | Guard: `tx.status == Processing` | ❌ Not yet implemented |
| I-04 | `Pending → Failed` and `Processing → Failed` are the only valid transitions for `fail_transaction` | Guard: `tx.status in [Pending, Processing]` | ❌ Not yet implemented |
| I-05 | `Completed` is a terminal state — no further transitions are permitted | Guard in all transition methods | ❌ Not yet implemented |
| I-06 | `Failed` is a terminal state — no further transitions are permitted | Guard in all transition methods | ❌ Not yet implemented |
| I-07 | `transaction_id` uniqueness — a second `register_callback` with the same `transaction_id` but a different `idempotency_key` must NOT overwrite the existing record | Idempotency key check; storage key is `StorageKey::Transaction(tx_id)` — a second write would overwrite. **Requires explicit duplicate `transaction_id` guard.** | ⚠️ Gap — see **F-07** |
| I-08 | The contract must be initialised before any state-mutating method can succeed | `is_initialised()` implicitly enforced via `get_admin()` / `get_relay_signer()` returning `NotInitialised` | ✅ Implicitly enforced |
| I-09 | `updated_at_ledger` must always be ≥ `created_at_ledger` | Ledger sequence is monotonically increasing | ✅ Guaranteed by Soroban runtime |
| I-10 | `stellar_tx_hash` must be empty string until `Completed` | Set in `register_callback`; populated only in `complete_transaction` | ❌ Not yet implemented (for completion half) |
| I-11 | `failure_reason` must be empty string until `Failed` | Set in `register_callback`; populated only in `fail_transaction` | ❌ Not yet implemented (for failure half) |

---

## 6. Storage Security

### Storage tier mapping and risks

| Key | Tier | Risk |
|-----|------|------|
| `StorageKey::Initialised` | Instance | Lost if contract instance is restored from archive — but `set_initialised` is called during `initialize()`, so restore would require re-initialisation. **Risk: none** (instance storage survives same-schema upgrades). |
| `StorageKey::Admin` | Persistent | Survives upgrades; only writable by `initialize()` and (TODO) `transfer_admin()`. **Risk: admin key compromise** — mitigated by multisig. |
| `StorageKey::RelaySigner` | Persistent | Same as Admin. |
| `StorageKey::Paused` | Instance | Survives upgrades. A paused contract that is upgraded remains paused. **Risk: none**. |
| `StorageKey::Transaction(id)` | Persistent | TTL extended on every read and write; bounded to `TRANSACTION_MIN_TTL_LEDGERS` (100 000 ledgers ≈ 1 week). If a record's TTL expires and is not extended, the record becomes unreadable (archived). **Risk: data availability** — active relay must regularly read/update active transactions. |
| `StorageKey::IdempotencyKey(key)` | Temporary | TTL of 18 000 ledgers (~24 h). After expiry, the key is invisible — a relay replaying a message after 24 h will not be deduplicated by the idempotency key alone. **Risk: late replay** — mitigated by transaction ID uniqueness (F-07 must be fixed). |

### Storage manipulation threats

- **Key collision:** All `StorageKey` variants are typed by the Soroban XDR
  codec. Collision between `Transaction(id)` and `IdempotencyKey(id)` for the
  same string value is impossible because they are different enum variants.
- **Storage exhaustion:** Each `register_callback` writes one persistent record
  (~500 B) and one temporary record (~50 B). At Soroban rent rates this is
  bounded by ledger fees; an attacker would pay proportionally. No unbounded
  write loop exists.
- **TTL manipulation:** TTL extension on `get_transaction` is bounded and
  caller-cost-metered. An attacker cannot extend TTL without paying fees.

---

## 7. Event Security

Events are written to the Stellar ledger and are immutable once included.
The two-topic convention `(symbol_short!("synapse"), symbol_short!("<name>"))`
is consistent across all emitters.

| Event | Topics | Emitted By | Status |
|-------|--------|-----------|--------|
| `EventInitialised` | `(synapse, init)` | `initialize()` | ✅ Implemented |
| `EventTransactionRegistered` | `(synapse, reg)` | `register_callback()` | ✅ Implemented |
| `EventContractUpgraded` | `(synapse, upgrade)` | `upgrade()` | ✅ Implemented |
| `EventPauseToggled` | `(synapse, pause)` | `pause()` / `unpause()` | ✅ Implemented |
| `EventStatusChanged` | `(synapse, status)` | status transitions | ❌ Not yet implemented |
| `EventTransactionCompleted` | `(synapse, done)` | `complete_transaction()` | ❌ Not yet implemented |
| `EventTransactionFailed` | `(synapse, fail)` | `fail_transaction()` | ❌ Not yet implemented |
| `EventAdminTransferred` | `(synapse, admin)` | `transfer_admin()` | ❌ Not yet implemented |

**Event security considerations:**

- Missing events on state transitions are a silent-failure risk for Phase 2/3
  subscribers. All `todo!()` event emitters must be implemented before audit.
- The `EventTransactionRegistered` event intentionally omits the
  `idempotency_key` to avoid leaking it on-chain. ✅ Correct by design.
- The `EventContractUpgraded` event includes the `new_wasm_hash`. Off-chain
  monitoring **must** subscribe to this event to detect unexpected upgrades.
- Symbol truncation: `symbol_short!` is limited to 9 bytes. All current
  symbols (`synapse` = 7, `upgrade` = 7, `status` = 6, etc.) are within
  limits. ✅ Verified.

---

## 8. Accepted Risks & Compensating Controls

### R-01: Single relay signer (hot key)

**Risk:** The relay signer is a single Stellar address whose private key is
held by the running off-chain relay service. If the relay service is
compromised, an attacker can forge `register_callback` calls or drive
arbitrary status transitions.

**Compensating controls:**
- The relay key should be rotated immediately if a breach is detected
  (`set_relay_signer()` — admin-gated).
- The admin can pause ingestion (`pause()`) to halt new registrations.
- Existing transactions cannot be deleted; only their status can be advanced
  or failed — the audit trail is immutable.
- The relay signer should be a dedicated key, not reused for any other purpose.
- A multi-relay design (multiple relay signers requiring M-of-N agreement) is
  a future phase consideration.

**Residual risk:** Medium. Accepted for Phase 1. Tracked as **R-01**.

---

### R-02: Admin-key is the root of all trust

**Risk:** The admin key controls `upgrade()`, `pause()`, `transfer_admin()`,
and `set_relay_signer()`. A compromised admin key allows arbitrary WASM
deployment (full contract replacement), making this the highest-severity
single point of failure.

**Compensating controls:**
- The admin **must** be a ≥3-of-5 multisig (Stellar native multisig or a
  DAO contract). This is a documented hard requirement in `README.md` and
  `DECISIONS.md`.
- All admin operations emit events; off-chain monitoring should alert on any
  `(synapse, upgrade)`, `(synapse, admin)`, or unexpected `(synapse, pause)`
  events.
- The admin key must not be the same as the deployer or relay signer key.
- A timelock on `upgrade()` (requiring a 24–48 h delay between scheduling
  and execution) is noted as a future enhancement in `DECISIONS.md §7`.

**Residual risk:** High if multisig is not enforced. Low if enforced. Accepted for Phase 1 with multisig requirement.

---

### R-03: No on-chain Horizon verification

**Risk:** The contract does not independently verify that a Stellar payment
actually occurred on Horizon. It trusts the relay's assertion that
`stellar_tx_hash` corresponds to a real settled transaction.

**Compensating controls:**
- The relay service performs Horizon verification before calling
  `complete_transaction()`.
- The `stellar_tx_hash` is stored immutably on-chain for post-hoc audit.
- A compromised relay is already addressed by R-01.

**Residual risk:** Low. Accepted by design — on-chain Horizon oracle is out
of scope for Phase 1.

---

### R-04: Idempotency window is finite (~24 h)

**Risk:** The temporary-storage idempotency key expires after ~18 000 ledgers
(~24 hours). A late replay of a webhook after this window will not be caught
by the idempotency key alone.

**Compensating controls:**
- The off-chain relay's Redis-based deduplication provides a first line of
  defence for duplicates arriving within the relay's own window.
- Fix **F-07** (transaction ID uniqueness guard) provides a persistent
  second-line defence on-chain regardless of TTL expiry.

**Residual risk:** Low after F-07 is fixed. Medium until then.

---

### R-05: In-place upgrade — no timelock

**Risk:** An admin (or compromised admin multisig) can upgrade the contract
WASM immediately, without a delay that would allow users to exit.

**Compensating controls:**
- Multisig requirement means M-of-N keys must sign the upgrade transaction.
- `EventContractUpgraded` is emitted; monitoring can detect and alert within
  seconds.
- A timelock enhancement is planned (see `DECISIONS.md §7`) but not in scope
  for Phase 1.

**Residual risk:** Medium. Accepted for Phase 1. Monitoring is mandatory.

---

## 9. Self-Review Findings

The following findings were identified during this self-review. Each is either
a known implementation gap (marked ❌ in sections above) or a specific
correctness/security issue. Each finding is rated by severity and linked to a
follow-up action.

| ID | Severity | Title | Description | Status |
|----|----------|-------|-------------|--------|
| **F-01** | Low | G-address validation is format-only | `validate_stellar_account` checks length == 56 and first byte == `G`. It does not validate the base32 checksum (last 2 bytes of a StrKey-decoded address). A carefully crafted 56-char string starting with `G` with an invalid checksum will pass. | Open — follow-up issue recommended |
| **F-02** | Medium | No guard against transferring admin to zero/uncontrolled address | `transfer_admin` (TODO) has no on-chain check that `new_admin` is a valid, non-null address. Transferring admin to an uncontrolled address permanently bricks privileged operations. | Open — add guard in implementation |
| **F-03** | Low | Single-step admin transfer | Admin transfer is a single atomic operation. A two-step pattern (nominate → accept) would require the new admin to prove key control before the transfer is finalised, preventing accidental or malicious mis-transfer. | Accepted risk for Phase 1; recommend follow-up issue for Phase 2 |
| **F-04** | Medium | No storage schema version for upgrade safety | There is no on-chain schema version stored. An upgrade that inadvertently changes `Transaction` struct layout or `StorageKey` variants will silently corrupt persistent storage. | Open — recommend adding a `SCHEMA_VERSION` constant checked by `upgrade()` |
| **F-05** | Low | Upgrade does not require contract to be paused | An upgrade can execute while `register_callback` calls are in-flight, creating a race condition if the new WASM changes callback processing semantics mid-flight. | Accepted risk for Phase 1; documented in `DECISIONS.md §7` |
| **F-06** | High | `get_status`, `is_duplicate`, and all status-transition methods are unimplemented | Five public entry points are `todo!()` stubs. The contract is not production-ready until these are implemented and tested. This is the primary implementation blocker for audit readiness. | Open — tracked by `TODO.md` |
| **F-07** | High | No persistent guard against `transaction_id` reuse after idempotency TTL expiry | If the same `transaction_id` is submitted with a different `idempotency_key` after the ~24 h idempotency window, `StorageClient::save_transaction` will overwrite the existing record. This violates the audit immutability guarantee. | Open — `register_callback` must check for an existing `StorageKey::Transaction(tx_id)` and return `DuplicateRequest` if present, regardless of idempotency key state. |
| **F-08** | Medium | `set_relay_signer` does not emit an event | Relay signer rotation is a sensitive operation (R-01). Without an on-chain event, monitoring systems cannot detect an unexpected rotation. | Open — must emit event in implementation |
| **F-09** | Low | `start_processing`, `complete_transaction`, `fail_transaction` accept an explicit `caller` argument | The caller address is passed in as a function argument rather than being derived from the signed transaction. The implementation must call `caller.require_auth()` to ensure the `caller` argument matches the actual transaction signer. Failing to do so would allow any `caller` address to be spoofed. | Open — critical to get right in implementation |
| **F-10** | Low | Asset issuer not validated against known trusted issuers | The contract validates that `asset_issuer` is a valid G-address but does not check it against an allowlist of trusted issuers. A relay could register callbacks for any asset. | Accepted risk for Phase 1 — asset filtering is an off-chain relay responsibility |

---

## 10. External Auditor Checklist

The following items must be complete and available before engaging an external
security auditor. This checklist is also the gating condition for mainnet
deployment.

### 10.1 Code & Documentation

- [ ] All `todo!()` stubs in `lib.rs`, `admin.rs`, `events.rs` are implemented
      and pass `make check` (fmt + clippy + test + wasm build).
- [ ] `THREAT_MODEL.md` (this document) is reviewed, merged, and up to date
      with the final implementation.
- [ ] `DECISIONS.md` is current and covers any design decisions made after
      the scaffold phase.
- [ ] `DEPLOYMENT.md` is validated against the actual testnet deployment.
- [ ] An **events schema doc** is produced listing all emitted events with
      their topic symbols, data struct fields, and the conditions under which
      each is emitted. (Currently implicit in `events.rs` — needs a dedicated
      `EVENTS.md` or section.)
- [ ] All self-review findings (F-01 through F-10) are either fixed or have
      a documented accepted-risk rationale with a linked follow-up issue.

### 10.2 Test Coverage

- [ ] All 20+ test skeletons in `tests.rs` are implemented (happy paths,
      auth failures, invalid inputs, idempotency, invalid state transitions).
- [ ] `test_pause.rs` full suite passes (currently 7 tests — all passing ✅).
- [ ] Test coverage report generated and reviewed. Target: 100% of public
      entry points covered by at least one positive and one negative test.
- [ ] Fuzz targets or property-based tests exist for `validation.rs` input
      guards (especially `validate_stellar_account` and `validate_amount`).

### 10.3 Testnet Deployment

- [ ] Contract deployed to Stellar testnet with a real multisig admin.
- [ ] Testnet contract ID documented and publicly accessible.
- [ ] End-to-end flow verified on testnet:
      `initialize → register_callback → start_processing → complete_transaction`.
- [ ] Emergency pause round-trip verified on testnet.
- [ ] Upgrade round-trip verified on testnet (upgrade to same WASM hash).

### 10.4 Operational Security

- [ ] Admin key is a ≥3-of-5 Stellar multisig (or DAO contract) — verified
      on testnet by requiring 3 signatures to call `pause()`.
- [ ] Relay signer key is a dedicated hot key, separate from admin and deployer.
- [ ] Off-chain monitoring is subscribed to `(synapse, upgrade)` and
      `(synapse, pause)` events with alerting configured.
- [ ] Key-rotation runbook documented: how to rotate relay signer, how to
      transfer admin, what to do if a key is compromised.

### 10.5 Artefacts to Provide to Auditor

- [ ] This document (`THREAT_MODEL.md`).
- [ ] Source code at a tagged commit (e.g., `v0.1.0-audit-candidate`).
- [ ] Compiled WASM artefact with SHA-256 hash matching the testnet deployment.
- [ ] Test suite output (`cargo test --verbose` log).
- [ ] Events schema document (`EVENTS.md` or equivalent).
- [ ] `DECISIONS.md` (upgrade strategy and trust model).
- [ ] `COST_MODEL.md` (storage rent model — relevant for storage exhaustion
      analysis).
- [ ] Testnet contract ID and a link to a sample `register_callback` →
      `complete_transaction` transaction on Horizon.
- [ ] List of all open findings from this self-review (section 9) with their
      resolution status.

### 10.6 Audit Scope Guidance for Auditor

Suggest the following as the primary focus areas for an external audit:

1. **Access control correctness** — verify that every state-mutating entry
   point enforces the correct role (admin vs relay) and that `require_auth()`
   is called on the right address.
2. **State-machine integrity** — verify that no invalid transition is reachable
   and that terminal states (`Completed`, `Failed`) are truly terminal.
3. **Idempotency and replay protection** — verify that F-07 is fixed and that
   there is no path to duplicate transaction creation.
4. **Upgrade safety** — verify that `upgrade()` cannot be called by a
   non-admin and that the post-upgrade storage state is consistent.
5. **Input validation completeness** — verify that all string fields passed
   to write methods are validated before storage, including fields in
   `start_processing`, `complete_transaction`, and `fail_transaction`.
6. **Event completeness** — verify that no state transition is silent (all
   emitters are implemented and called).

---

*This document was produced as part of issue #40 — pre-audit threat model and
security-audit readiness checklist. It is a living document and should be
updated as the implementation matures toward audit candidacy.*
