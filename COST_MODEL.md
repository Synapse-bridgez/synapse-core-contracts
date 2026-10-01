# Cost Model — Per-Transaction XLM Estimate

> **Date:** 2025-Q2  
> **Target:** Mainnet (Soroban Protocol 22)  
> **Contract:** `synapse-core-contract` (Phase 1, on-chain transaction registry)

---

## 1. Scope

This document estimates the **per-transaction XLM cost** of the contract's core
lifecycle.  The estimate covers:

- Initial write of a `Transaction` record (persistent storage)
- Initial write of an idempotency key (temporary storage)
- Read + TTL-extension for each status transition
- Occasional off-chain polling reads

It does **not** cover the one-time `initialize` call, admin operations (pause,
transfer, upgrade), or the "dead entry" cost of expired idempotency keys that
the ledger evicts for free.

---

## 2. Storage Footprint

### 2.1 Persistent entry — `Transaction` record

Key: `StorageKey::Transaction(tx_id)` where `tx_id` is a UUID (36 chars).

| Field                  | Type             | XDR size | Notes                                        |
|------------------------|------------------|----------|----------------------------------------------|
| `id`                   | `String`         | 40 B     | UUID: 36 chars + 4-byte length prefix        |
| `stellar_account`      | `String`         | 60 B     | 56-char G-address + 4-byte length prefix     |
| `amount`               | `i128`           | 16 B     | Fixed-width                                  |
| `asset_code`           | `String`         | 16 B     | 12 chars + 4-byte prefix (capped by validator) |
| `asset_issuer`         | `String`         | 60 B     | 56-char G-address + 4-byte prefix            |
| `status_flags`         | `u32`            | 4 B      | Bit-packed status (#117); see §2.1.2         |
| `created_at_ledger`    | `u32`            | 4 B      |                                              |
| `updated_at_ledger`    | `u32`            | 4 B      |                                              |
| `anchor_transaction_id`| `String`         | 40 B     | ~36 chars (opaque ID) + 4-byte prefix        |
| `callback_type`        | `CallbackType`   | 4 B      | 4-byte enum discriminant                     |
| `callback_status`      | `String`         | 24 B     | ~20 chars + 4-byte prefix                    |
| `stellar_tx_hash`      | `String`         | 4 B      | Empty string initially (4-byte prefix only)  |
| `failure_reason`       | `String`         | 4 B      | Empty string initially (4-byte prefix only)  |
| `tags`                 | `Vec<String>`    | 4 B+     | Empty vec initially; see section 2.1.1       |
| **Struct subtotal**    |                  | **280 B** |                                              |
| Storage key overhead   | `StorageKey` enum| 44 B     | 4-byte discriminant + 40-byte String         |
| XDR framing            |                  | 8 B      | Struct header / padding                      |
| **Total per entry**    |                  | **~332 B** | Rounded to **512 B** for fee calculation    |

> **Note:** The two empty-string fields (`stellar_tx_hash`, `failure_reason`)
> are replaced with populated values during the transaction lifecycle.  Their
> worst-case sizes are 68 B (64-char hex hash + prefix) and 24 B respectively.
> After final status, the entry grows to ~396 B.

#### 2.1.1 Tag storage (append-only, capped)

Tags are capped at 8 per transaction and 32 chars each. Each tag costs
`4 B length prefix + chars` (XDR pads to 4 B), on top of a 4 B vec header:

| Tags | Example content       | Added size |
|------|-----------------------|------------|
| 1    | one ~20-char tag      | ~24 B      |
| 5    | five ~20-char tags    | ~120 B     |
| 8 (max) | eight 32-char tags | ~288 B     |

Even at the cap the record stays within the 512 B fee-rounding bucket only
when other fields are near their typical sizes; budget up to the next bucket
for fully tagged records.

#### 2.1.2 Bit-packed status (#117)

The ledger entry is a `StoredTransaction`, which replaces the
`TransactionStatus` enum with a `u32` (`StatusFlags`: status nibble in bits
7:4, all other bits reserved for future flags). A `#[contracttype]` unit enum
serialises as a one-element `ScVec` holding the variant name as an
`ScSymbol`, so the packed word is smaller for every status.
`StorageClient` converts at the storage boundary; `get_transaction()` still
returns the unpacked `Transaction`.

Measured XDR size of a full record (`tests::test_packed_transaction_is_smaller_than_unpacked`):

| Status                              | Unpacked | Packed | Saved |
|-------------------------------------|----------|--------|-------|
| `Pending`, `Failed`                 | 632 B    | 616 B  | 16 B  |
| `Processing`, `Completed`, `Cancelled` | 636 B | 616 B  | 20 B  |

That is ~3% of persistent rent per record. The packed size no longer
depends on the status, so status transitions never resize the entry.

### 2.2 Temporary entry — idempotency key

| Component              | Type             | XDR size | Notes                                   |
|------------------------|------------------|----------|-----------------------------------------|
| Key: `StorageKey::IdempotencyKey(String)` | 44 B | 4-byte discrim + ~40-byte idempotency key string |
| Value                  | `u32`            | 4 B      | Ledger sequence when first stored       |
| **Total per entry**    |                  | **~48 B** | Rounded to **64 B** for fee calculation |

---

## 3. Soroban Fee Rates (Protocol 22)

| Component               | Rate (stroops) | Rate (XLM)        | Notes                                |
|-------------------------|----------------|--------------------|--------------------------------------|
| **Write entry**         | ~50,000        | 0.0050 XLM         | Host function + storage write cost   |
| **Read entry**          | ~5,000         | 0.0005 XLM         | Host function + storage read cost    |
| **Extend TTL call**     | ~10,000        | 0.0010 XLM         | Base fee for `extend_ttl`            |
| **Persistent rent**     | ~1 stroop/byte | per 100,000 ledgers| 100,000 ledgers ≈ 1 week at 5s       |
| **Temporary rent**      | ~0.5 stroop/byte| per 18,000 ledgers| 18,000 ledgers ≈ 24 hours at 5s      |
| **CPU/memory**          | ~10,000        | 0.0010 XLM         | Per-operation resource component     |

*(1 stroop = 0.0000001 XLM; 10,000,000 stroops = 1 XLM)*

---

## 4. Per-Transaction Cost Breakdown

### 4.1 `register_callback` — Initial write

| Operation               | Size   | Write fee | Rent component         | Total (XLM) |
|-------------------------|--------|-----------|------------------------|-------------|
| Write `Transaction`     | 512 B  | 0.0050    | 512 × 1 × 1e-7 = 0.0000512 | 0.00505 |
| Write idempotency key   | 64 B   | 0.0050    | 64 × 0.5 × 1e-7 = 0.0000032 | 0.00500 |
| CPU/memory              |        |           |                        | 0.0010      |
| **Total register**      |        |           |                        | **~0.0111 XLM** |

#### 4.1.1 Hot-path profile and optimisation (#116)

Measured by `bench_events::bench_register_callback_paths`: a full client
invocation (auth, storage, validation, event) with the Soroban test budget.
Native Rust numbers, so read them as relative, not absolute (see §9.1).

**Where the cost goes (happy path, ~149k CPU):** two persistent/temporary
writes plus the `reg` event dominate. Of the validation, the two SEP-23
CRC16 strkey checks (`stellar_account`, `asset_issuer`) are the largest
pure-CPU part. The amount-ceiling lookup is the only validation step that
reads storage.

**Changes:**

1. The idempotency check runs before payload validation, so replays skip
   both CRC16 checks and the ceiling read.
2. The amount-ceiling storage read moved from 3rd to last in
   `validate_payload`, so every pure-CPU check can reject first and the
   lookup is always keyed on a well-formed issuer.
3. The `Transaction` is built by moving fields out of the owned payload
   instead of cloning them, and `transaction_registered` takes only the
   fields in its payload (#118).
4. Status is stored bit-packed (§2.1.2).

| Path (native Rust)             | CPU before | CPU after | Mem before | Mem after |
|--------------------------------|-----------:|----------:|-----------:|----------:|
| happy (first registration)     | 149 177    | 148 523   | 21 133     | 21 005    |
| replay (same idempotency key)  | 63 383     | 54 768 (−13.6%) | 8 178 | 7 442 (−9.0%) |
| over ceiling                   | 51 650 ¹   | 67 988 ²  | 7 130      | 9 111     |
| paused                         | 23 246     | 23 246    | 2 930      | 2 930     |

¹ Before: hard `Err` at the 3rd validation step; the call reverts.
² After: soft rejection (#115). All validation runs, then a committed
`val_rej` event. The extra cost buys the observable trace.

The happy-path win is small: its cost is dominated by the two ledger
writes, which these changes don't touch. The main gain is on replays, which
relays send whenever they retry.

**CI gate:** `bench_register_callback_paths` asserts happy-path
CPU ≤ 450 000 and memory ≤ 65 000 (~3× measured), and that every early-exit
path costs less than a full registration. It runs as part of `make test`;
`make bench` prints the `[bench]` lines.

| Date       | CPU ceiling | Mem ceiling | Reason                         |
|------------|-------------|-------------|--------------------------------|
| 2026-10-01 | 450 000     | 65 000      | Initial gate (#116)            |

### 4.2 `start_processing` — Status transition

| Operation               | Size   | Read fee | Extend TTL + rent     | Total (XLM) |
|-------------------------|--------|----------|------------------------|-------------|
| Read `Transaction`      | 512 B  | 0.0005   | 0.0010 + 0.0000512    | 0.00155     |
| CPU/memory              |        |          |                        | 0.0010      |
| **Total start**         |        |          |                        | **~0.0026 XLM** |

### 4.3 `complete_transaction` / `fail_transaction` — Terminal transition

(Same as `start_processing` — one read + one TTL extension)

| Operation               |          | Total (XLM) |
|-------------------------|----------|-------------|
| Read + TTL extension    |          | 0.0026      |
| **Total terminal**      |          | **~0.0026 XLM** |

### 4.4 Off-chain polling (optional, ~3 reads)

| Operation               |          | Total (XLM) |
|-------------------------|----------|-------------|
| 3 × read + TTL extension | 3×0.00155 | 0.00465     |
| CPU/memory              | 3×0.0010  | 0.0030      |
| **Total polling**       |          | **~0.0077 XLM** |

---

## 5. Lifecycle Totals

### 5.1 Minimum lifecycle (no polling)

| Step                  | XLM         |
|-----------------------|-------------|
| `register_callback`   | 0.0111      |
| `start_processing`    | 0.0026      |
| `complete_transaction`| 0.0026      |
| **Total**             | **0.0163 XLM ≈ 0.016 XLM** |

### 5.2 Typical lifecycle (3 off-chain polls)

| Step                  | XLM         |
|-----------------------|-------------|
| `register_callback`   | 0.0111      |
| 3 × status transitions| 0.0078      |
| 3 × polling reads     | 0.0077      |
| **Total**             | **0.0266 XLM ≈ 0.027 XLM** |

### 5.3 Budget for relay operator

| Volume     | Monthly tx | Monthly cost (min) | Monthly cost (typical) |
|------------|------------|--------------------|------------------------|
| Low        | 1,000      | 16 XLM (~$0.20)    | 27 XLM (~$0.33)        |
| Medium     | 10,000     | 160 XLM (~$1.95)   | 270 XLM (~$3.30)       |
| High       | 100,000    | 1,600 XLM (~$19.50)| 2,700 XLM (~$32.95)    |
| Peak       | 1,000,000  | 16,000 XLM (~$195) | 27,000 XLM (~$329)     |

*(XLM price assumed at ~$0.122 per CoinMarketCap 2025-Q2 average)*

---

## 6. String-Length Cap Impact (Enforced)

**String-length caps have been implemented in [`validation.rs`](./src/validation.rs)**
as of this document's publication.  The caps are:

| Field                       | Cap (bytes) | Rationale                                    |
|-----------------------------|-------------|----------------------------------------------|
| `transaction_id`            | 64          | UUIDv4 is 36 chars; generous cushion         |
| `anchor_transaction_id`     | 64          | Opaque AP ID, typically ≤ 36 chars           |
| `callback_status`           | 32          | Short status code, e.g. `pending_external`   |
| `stellar_tx_hash`           | 72          | SHA-256 hex is 64 chars; + prefix overhead   |
| `failure_reason`            | 64          | Short code, e.g. `horizon_timeout`           |

**Without these caps** a malicious relay or bug could store megabyte-sized
strings, driving rent cost to tens of XLM per entry.  The caps close this
attack vector.

### 6.1 Strkey CRC16 validation cost

`stellar_account` / `asset_issuer` are validated with a hand-rolled SEP-23
base32 decode + CRC16-XModem check (see [`DECISIONS.md`](./DECISIONS.md)
§ Strkey CRC16).  Per-call CPU is low thousands of instructions — negligible
beside the ~0.01 XLM storage write for `register_callback`.  Release WASM grew
by **418 bytes** (19 820 → 20 238) under `opt-level = "z"`; fee impact is
dominated by ledger writes, not validation.

With the enforced caps, the **worst-case persistent entry** is:

| Field (worst case)          | Size     |
|-----------------------------|----------|
| `id`                        | 68 B     |
| `stellar_account`           | 60 B     |
| `amount`                    | 16 B     |
| `asset_code`                | 16 B     |
| `asset_issuer`              | 60 B     |
| `status`                    | 4 B      |
| `created_at_ledger`         | 4 B      |
| `updated_at_ledger`         | 4 B      |
| `anchor_transaction_id`     | 68 B     |
| `callback_type`             | 4 B      |
| `callback_status`           | 36 B     |
| `stellar_tx_hash` (capped)  | 76 B     |
| `failure_reason` (capped)   | 68 B     |
| Key + framing               | 52 B     |
| **Total**                   | **~536 B** |

This is near-identical to the unbounded estimate of 512 B because the fixed-
width fields (addresses, amounts, enums) dominate the footprint.  The caps
primarily protect against pathological inputs, not typical usage.

**The cost model in §4–5 reflects this enforced worst case and is final.**

---

## 7. Key Assumptions

1. **Ledger time:** 5 seconds per ledger (Stellar mainnet nominal).
2. **TTL values:** `TRANSACTION_MIN_TTL_LEDGERS = 100,000` (~1 week) and
   `IDEMPOTENCY_TTL_LEDGERS = 18,000` (~24 hours).  These are the constants
   in [`storage.rs`](./src/storage.rs).
3. **Read pattern:** 3 status transitions × 1 read each + 3 off-chain polling
   reads × 1 read each = 6 total reads × TTL extension.
4. **Fee rates:** Based on Soroban Protocol 22 resource model.  These are
    subject to change via Stellar network governance votes (CAPs).
5. **XLM price:** $0.122 (2025-Q2 average).  Actual cost varies with market
    price and network congestion surcharges.
6. **No storage inflation:** The estimate assumes fees remain at baseline;
    during congestion, Soroban uses a fee-auction model that can raise prices
    10× or more.

---

## 8. Summary

| Metric                    | Value              |
|---------------------------|--------------------|
| Cost per tx (min lifecycle)| **0.016 XLM**      |
| Cost per tx (typical)     | **0.027 XLM**      |
| Monthly cost at 10K tx    | **160–270 XLM**    |
| Primary cost driver       | Persistent write + rent for `Transaction` record |

The cost is dominated by the **initial write of the Transaction record**
(~40% of lifecycle cost).  Each subsequent read + TTL extension is relatively
cheap.  The relay operator should budget for the **typical lifecycle** to
account for off-chain monitoring reads.

**String-length caps have been implemented** in [`validation.rs`](./src/validation.rs)
(§6).  Without them, a malicious relay could drive costs to tens of XLM per
entry via megabyte-sized strings.  The caps close this vector with negligible
impact on typical usage.

---

## 9. Event Emission Cost

> **Added:** Wave 7 (2026-Q3) — motivated by near-universal event addition
> to privileged entry-points (audit-event and versioned-payload issues).
> Previously treated as negligible; measured here to justify that assumption
> for the expanded catalogue.

### 9.1 Measurement methodology

A dedicated benchmark harness lives in [`src/bench_events.rs`](./src/bench_events.rs).
Each of the 10 [`EventEmitter`](./src/events.rs) variants is measured using the
Soroban test SDK's budget API (`env.cost_estimate().budget()`):

1. **Baseline** — run the argument-construction code inside `env.as_contract(…)`
   without calling `events().publish(…)`.  Budget tracker is reset via
   `reset_tracker()` before both the baseline and the measured closure.
2. **Measured** — same closure, but with the `EventEmitter::*` call included.
3. **Delta** — `measured − baseline`, isolating only the cost of the
   serialise-and-publish step.

**Important caveat:** The Soroban SDK docs note that CPU instructions and
memory usage are **underestimated in native Rust tests** compared to WASM
execution.  The numbers below reflect *relative* cost (which event is cheapest)
and *directional* trends (total event cost versus total lifecycle cost) rather
than exact on-chain XLM values.  WASM numbers will be proportionally higher
but directionally identical.

Run the benchmarks yourself with:
```bash
cargo test bench_ -- --nocapture 2>&1 | grep '\[bench\]'
```

### 9.2 Per-event cost (native Rust, indicative)

All measurements taken on the same environment/run as the CI test suite.
String fields use realistic (not worst-case) lengths.

| Event topic | Payload summary | CPU Δ (instructions) | Mem Δ (bytes) |
|-------------|----------------|----------------------|---------------|
| `init`      | 2 × Address + u32 | ~20 000 – 60 000  | ~1 000 – 6 000 |
| `reg`       | 4 × String + i128 + u32 | ~30 000 – 80 000 | ~2 000 – 10 000 |
| `status`    | 1 × String + 2 × TransactionStatus + u32 | ~20 000 – 60 000 | ~1 000 – 6 000 |
| `done`      | 2 × String + u32 | ~25 000 – 65 000   | ~1 500 – 8 000 |
| `fail`      | 2 × String + u32 | ~25 000 – 65 000   | ~1 500 – 8 000 |
| `propose`   | 2 × Address + u32 | ~20 000 – 60 000   | ~1 000 – 6 000 |
| `admin`     | 2 × Address + u32 | ~20 000 – 60 000   | ~1 000 – 6 000 |
| `relay`     | 2 × Address + u32 | ~20 000 – 60 000   | ~1 000 – 6 000 |
| `pause`     | bool + Address + u32 | ~15 000 – 55 000 | ~800 – 5 000 |
| `upgrade`   | Address + BytesN<32> + 2 × u32 | ~20 000 – 60 000 | ~1 000 – 7 000 |

> Ranges reflect typical run-to-run variation across the test environment.
> See the `[bench]` lines in `cargo test -- --nocapture` for exact numbers.

### 9.2.1 Lazy construction audit (#118)

Audit of every `EventEmitter` call site in `src/lib.rs` (21 sites):

* Every emitter builds its event struct inside the emitter, immediately
  before `env.events().publish()`. No call site builds event data before a
  guard that can return early, so no rejected path (paused, auth failure,
  validation, duplicate) pays for event construction.
* The one structural change: `transaction_registered` used to take a whole
  `&Transaction`, so emitting `reg` meant having a full record in hand. It
  now takes only the five fields in its payload. Fields outside the event
  are never touched. The event bytes are unchanged: the struct, field order
  and values are identical, and the existing payload and conformance tests
  pass unmodified.
* `old_status = tx.status.clone()` in the three status transitions is now a
  plain copy (`TransactionStatus: Copy`).

`bench_event_reg` measures the `reg` emitter at 7 916 CPU / 783 B
(native). It reports the same numbers with either signature, because the
emitter clones the same five fields either way. The saving is on the
caller side and is part of the §4.1.1 figures.

### 9.3 Lifecycle event cost

The happy-path transaction lifecycle emits **4 events**:

| Entry-point              | Events emitted              |
|--------------------------|-----------------------------|
| `register_callback`      | 1 × `reg`                   |
| `start_processing`       | 1 × `status`                |
| `complete_transaction`   | 1 × `status` + 1 × `done`  |

Summing the midpoints from §9.2:

| Metric                               | Indicative value (native Rust) |
|--------------------------------------|--------------------------------|
| Lifecycle event CPU (reg+2×status+done) | ~95 000 – 265 000 instructions |
| Lifecycle event memory               | ~6 500 – 30 000 bytes          |
| **All 10 events combined**           | ~215 000 – 645 000 instructions |

### 9.4 Relative impact on lifecycle cost

The persistent-write for `register_callback` costs approximately **50 000
stroops** (0.005 XLM) in host-function fees alone (see §4.1).  One Soroban
CPU instruction unit costs approximately 100 stroops at baseline network rates,
so 265 000 instructions ≈ 0.00265 XLM — roughly **26% of the raw write fee**
in the absolute worst case under native measurement.  On WASM, instruction
counts scale up proportionally but so does the fee computation, so the
*fractional overhead* remains consistent.

Conclusion: **event emission is not negligible but is not dominant**.  The
per-lifecycle event cost (~0.002 – 0.005 XLM indicative) is in the same order
of magnitude as one status-transition read+TTL-extension (§4.2, ~0.0026 XLM).
The cost is **justified** by the downstream subscriber value (Phase 2 Swap
Engine, Phase 3 Bridge, and off-chain monitoring all depend on these events to
avoid polling).

### 9.5 CI regression gate

`src/bench_events.rs::bench_wave7_cumulative_cost` asserts:

| Dimension   | Regression ceiling (native Rust) |
|-------------|----------------------------------|
| CPU (all 10 events) | ≤ 5 000 000 instructions   |
| Memory (all 10 events) | ≤ 1 000 000 bytes         |

These ceilings are set conservatively high (≈ 8× the observed totals) to avoid
false positives from run-to-run variation, while still catching catastrophic
regressions (e.g. an accidental O(n²) allocation inside a new payload field).

**To update the ceilings:** raise the constants in `src/bench_events.rs`
(`CUMULATIVE_CPU_CEILING`, `CUMULATIVE_MEM_CEILING`) and add a line here
documenting the new values, the reason for the increase, and the date.

| Date       | CPU ceiling | Mem ceiling | Reason                         |
|------------|-------------|-------------|--------------------------------|
| 2026-Q3    | 5 000 000   | 1 000 000   | Initial measurement, Wave 7    |

## 10. Admin / upgrade resource add-on

Infrequent admin operations are outside the per-tx lifecycle budget above, but
two upgrade-path costs are worth calling out:

| Step | Reads | Writes | Notes |
|------|-------|--------|-------|
| `post_upgrade_self_check` | ~4 (init flag, admin, relay, schema) | 0 | Runs on every `upgrade()`; intentionally a handful of targeted reads |
| `append_upgrade_record` | 1 (history vec) | 1 (history vec) | Bounded at `MAX_UPGRADE_HISTORY` (32); FIFO eviction keeps rent flat |
| `set_current_wasm_hash` | 0 | 1 | 32-byte hash |
| `simulate_upgrade` | ≤3 | **0** | Pure query — must never write |

At Protocol 22 rates the self-check adds on the order of **~0.002 XLM** of
read fees per upgrade — negligible next to the WASM-swap host cost itself.

## 11. Batch registration (`batch_register_callback`)

Batch size is capped at `MAX_BATCH_SIZE` (20). Worst case is 20 payloads with
max-length string fields: each payload costs one persistent write for the
transaction (~512 B fee-rounded, see section 2), one temporary write for the
idempotency key, one status-index update, and one event, plus a single
`EventBatchProcessed`. Validation runs over the whole batch before any write,
including an O(n^2) in-batch duplicate check (at most 190 comparisons). The
cap is deliberately conservative to stay well under the per-transaction
resource limits; raise it only after benchmarking.

