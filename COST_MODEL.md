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
| `status`               | `TransactionStatus` | 4 B   | 4-byte enum discriminant                     |
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
§ Strkey CRC16).  Metered on the release WASM (§12) each strkey check costs
~30k instructions after #120's nibble-table CRC (~55k with the original
bitwise loop) — still negligible beside the ~0.01 XLM storage write for
`register_callback`, but the largest guest-side cost in validation, which is
why `validate_payload` runs it last (§12.5).  Release WASM grew
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

Two limits apply, both checked before any storage write (#173):

### 11.1 Resource-budget check (#173)

The count cap alone does not bound cost: 20 items with long strings (the
idempotency key is uncapped) can still approach Soroban's per-transaction
limits. Before any write, `batch_register_callback` computes a conservative,
O(n) byte estimate from `String::len()` alone
(`Validator::estimate_batch_cost`) and rejects the whole batch with
`BatchBudgetExceeded` if either figure exceeds its budget:

| Estimate | Per item | Per batch | Budget |
|----------|----------|-----------|--------|
| Write bytes | 256 + 2×`transaction_id` + `stellar_account` + `asset_code` + `anchor_transaction_id` + `asset_issuer` + `callback_status` + `idempotency_key` | — | 32 768 (½ of the lowest mainnet per-tx write limit, 65 536) |
| Event bytes | 128 + `transaction_id` + `stellar_account` + `asset_code` + `anchor_transaction_id` | 128 + first and last `transaction_id` | 8 192 (½ of the 16 384-byte per-tx events limit) |

Strings are weighted by how many times they are written: `transaction_id` is
in the storage key, the record, and the `reg` event; account, asset code and
anchor id are in the record and the event. The fixed overheads over-count XDR
framing on purpose. A false reject is acceptable; a mid-execution resource
failure is not.

With every capped field at its maximum, a full 20-item batch estimates at
16 000 + Σ`idempotency_key` write bytes and 6 736 event bytes, so every
legitimate batch fits. The budget only rejects batches with unusually large
idempotency keys (or over-long fields, which it catches before per-item
validation runs). Tests in `src/test_batch_budget.rs` pin the exact boundary:
a batch at exactly 32 768 estimated write bytes succeeds, and one byte more
is rejected with no storage write and no events.

## 12. Resource-usage regression gate (#119)

`.github/workflows/resource-gate.yml` builds the release WASM and runs
`scripts/check_resource_budget.sh`, which meters every hot entry point
(`src/bench_resources.rs`) and compares against the committed
[`resource_baseline.toml`](./resource_baseline.toml). The build fails if any
scenario's `cpu_insns` or `mem_bytes` exceeds its baseline by more than
`threshold_pct` (initially **15%**).

### 12.1 What is measured

* **Release WASM, not native Rust.** Native tests run contract code as plain
  Rust, so the budget sees only host calls; strkey decoding, CRC16 and any
  other guest-side loop cost nothing there. Registering the built `.wasm`
  meters every executed instruction, as validators do.
* **Net of invocation overhead.** Every call pays a fixed ~3.0M-instruction /
  ~1.77 MB cost to instantiate the VM for the ~53 KB module. That is recorded
  once as `invocation_overhead` (a trivial `health()` call — catches WASM-size
  bloat), and every other scenario is stored net of it, so a 15% threshold
  applies to the entry point's own work.
* **One fresh env per scenario** — preconditions (register, start processing,
  propose admin, …) are set up unmetered; only the call under test is metered.
* **Rejection paths.** `register_callback_reject_*` meter each validation
  failure so the cost of turning away malformed input is tracked (#120).

### 12.2 Variance and the threshold

Soroban metering is a deterministic cost model, not wall-clock timing:
repeated local runs and CI runs of the same WASM produce identical numbers
(0.0% delta), so runner noise does not apply. What *does* move the numbers is
a different WASM — including one built by a different `rustc`. The workflow
therefore pins `RUST_TOOLCHAIN` to the version the baseline was generated
with, and the script prints the WASM's size and SHA-256 prefix so a mismatch
is easy to spot. The 15% threshold is deliberately loose for a first rollout;
given zero measured variance it can be tightened (e.g. to 5%) once the team
is comfortable with the update workflow below.

### 12.3 Gate self-test

The workflow proves the gate is not vacuous: `--inject register_callback:25`
inflates one measurement by 25% and **must** fail (`--expect-fail`), while
`--inject register_callback:10` stays under the threshold and must pass. The
comparison logic itself (at/above threshold, missing/stale entries, baseline
round-trip) is unit-tested in every `cargo test`.

### 12.4 Updating the baseline

When a change legitimately needs more (or now needs less) budget:

1. Build with the pinned toolchain:
   `rustup run <RUST_TOOLCHAIN> cargo build --target wasm32-unknown-unknown --release`
2. Regenerate: `scripts/check_resource_budget.sh --update`
3. Commit `resource_baseline.toml` in the same PR, and state in the PR
   description which scenarios moved, by how much, and why.
4. Adding a new scenario to `src/bench_resources.rs` requires the same step —
   the gate reports `MISSING` for scenarios without a baseline entry and
   `STALE` for baseline entries without a scenario.

When bumping `RUST_TOOLCHAIN` in the workflow, regenerate the baseline with
the new toolchain in the same PR.

A failure looks like:

```text
Resource-usage gate failed (#119): 1 finding(s) against resource_baseline.toml

  REGRESSION  register_callback: cpu_insns 442300 vs baseline 353840 (+25.0%; threshold +15%)
```

### 12.5 Validation short-circuit (#120)

`Validator::validate_payload` now runs checks cheapest-first — `amount`, then
the four host-`len()` checks, then `asset_code`, and the two strkey
verifications last — and strkey verification rejects a non-`G` prefix before
the base32 decode. CRC16 uses a 16-entry nibble table (+49 B WASM) instead of
eight shift/branch rounds per byte. Accept/reject outcomes are unchanged for
every combination of invalid fields (exhaustively tested); only the error
reported first changes when several fields are invalid.

Net `cpu_insns` per call, release WASM (`resource_baseline.toml`):

| Scenario | Before | After | Δ |
|----------|-------:|------:|--:|
| reject: `amount = 0` | 110 115 | 56 662 | −48.5% |
| reject: empty `idempotency_key` | 166 174 | 57 094 | −65.6% |
| reject: `callback_status` too long | 168 406 | 59 326 | −64.7% |
| reject: lowercase `asset_code` | 111 889 | 61 100 | −45.3% |
| reject: `stellar_account` CRC mismatch | 110 115 | 90 397 | −17.9% |
| reject: `asset_issuer` CRC mismatch (worst case) | 165 742 | 119 294 | −28.0% |
| `register_callback` (accepted) | 353 840 | 304 728 | −13.8% |
| `register_callback` (idempotent replay) | 190 807 | 141 695 | −25.7% |

### 12.6 Hot-path allocation audit (#121)

**Guest heap.** The release WASM is byte-identical with and without
`soroban-sdk`'s `alloc` feature: no contract code path allocates on the guest
heap, so the allocator is never linked. The existing fixed-capacity buffers
(`[u8; 56]` strkey, `[u8; 12]` asset code, `[u8; 35]` decode output) already
live on the stack and match their validation caps exactly; boundary tests at
cap−1 / cap / cap+1 are in `src/tests_hot_path.rs`. `alloc` stays enabled
(out of scope to remove).

**Host objects.** On Soroban the real allocation cost of these entry points
is host-side: every `String`, `Vec` and `Map` the contract creates, including
each `StorageKey` passed to a storage call (encoded to a new host
`Vec [Symbol, payload]` every time).

| Site | Verdict | Change |
|------|---------|--------|
| `StorageKey` re-encoded for `get`/`set` then `extend_ttl` (transaction, idempotency key) | Avoidable | Encode once (`storage::encode_key`), reuse the `Val` — identical ledger key |
| Two `String::from_str(&env, "")` in `register_callback` | Avoidable | One object, second use clones the handle |
| `assert_is_relay_or_admin` always reads admin **and** relay | Avoidable for relay callers | Check relay first; admin is read only if needed |
| `Transaction` → host `Map` on save, event structs → host `Map` on publish | Necessary | — (ledger / event encoding) |
| Handle `clone()`s of `String`/`Address` | Free | — (copies a 64-bit handle, no allocation) |
| Wave 2 bond/unbond/param storage | Not hot | Left as is |

Net cost per call, release WASM:

| Scenario | cpu_insns before → after | mem_bytes before → after |
|----------|-------------------------:|-------------------------:|
| `start_processing` | 255 476 → 208 035 (−18.5%) | 19 032 → 17 689 (−7.0%) |
| `complete_transaction` | 273 247 → 225 806 (−17.3%) | 20 057 → 18 714 (−6.6%) |
| `fail_transaction` | 268 135 → 220 694 (−17.6%) | 19 410 → 18 067 (−6.9%) |
| `get_transaction` | 100 746 → 88 161 (−12.4%) | 5 682 → 5 447 (−4.1%) |
| `register_callback` | 304 728 → 277 268 (−9.0%) | 18 463 → 17 886 (−3.1%) |
| `invocation_overhead` | 3 023 843 → 2 997 803 (−0.9%) | 1 770 917 → 1 768 066 (−0.2%) |

The release WASM also shrank from 53 475 to 53 432 bytes.

## 13. Release WASM size gate (#122)

`scripts/check_wasm_size.sh` (CI: `.github/workflows/resource-gate.yml`;
local: `make wasm-size`) measures `make wasm`'s output — the same release
profile (`opt-level = "z"`, `lto = true`, `strip = "symbols"`,
`panic = "abort"`) that `DEPLOYMENT.md` uploads — against
[`wasm_size.toml`](./wasm_size.toml).

### 13.1 Network limit

Read from the live network config via RPC `getLedgerEntries` on 2026-09-29;
mainnet and testnet are identical:

| Setting | Value | Relevance |
|---------|------:|-----------|
| `contract_max_size_bytes` (ConfigSettingID 0) | **131 072** | Hard cap on an uploaded WASM — the binding limit |
| `txMaxSizeBytes` (bandwidth, ID 5) | 132 096 | Upload tx envelope must also fit |
| `txMaxWriteBytes` (ledger cost, ID 2) | 132 096 | Upload writes the code entry |

Re-check after protocol upgrades (`stellar network settings`, or the same RPC
query) and update `network_limit_bytes` / `ceiling_bytes` if they change.

### 13.2 Checks

| Check | Setting | Fails when |
|-------|---------|-----------|
| Regression | `baseline_bytes`, `max_growth_pct = 5` | size > baseline × 1.05 |
| Ceiling | `ceiling_bytes = 98 304` | size > 96 KiB (75% of the network limit, leaving 32 KiB headroom for future in-place upgrades) |

At introduction the WASM is **53 432 bytes** — 54% of the ceiling, 41% of
the network limit — so no size-reduction follow-up is needed.

Failure output names the delta, e.g.:

```text
REGRESSION: release WASM grew 16429 bytes (+30.7%),
  53432 -> 69861 bytes; allowed growth is +5% (max 56103 bytes).
```

### 13.3 Self-test fixtures

CI builds the contract with `--features size-gate-fixture`, which links a
deliberately bloated dummy export (`src/size_gate_fixture.rs`, a 16 KiB
pseudo-random table) and asserts the gate **fails** on it; it also asserts
the real WASM fails an artificially low `--ceiling 50000`. The feature is
wasm-only and never part of a deployable build.

### 13.4 Accepting intended growth

Run `scripts/check_wasm_size.sh --update` (with the workflow's pinned
toolchain — size depends on `rustc`), commit `wasm_size.toml`, and state the
growth and its reason in the PR. Raising `ceiling_bytes` itself needs a
separate, explicit decision since it eats into upgrade headroom.
