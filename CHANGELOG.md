# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
as exposed by `SynapseCoreContract::version()`.

**Subscriber teams (Phase 2 / Phase 3):** prefer the
[Event schema](#event-schema) section when diffing releases — it is maintained
separately from general code changes. Full topic/field contracts live in
[`EVENTS.md`](./EVENTS.md).

---

## [Unreleased]

> **Restoration note (#199).** Merges #176–#197 dropped most of their `src/`
> changes, and `main` did not compile from #176 until #199 was fixed. The
> features below were re-implemented against the current API. Items that were
> announced here but never existed in code have been removed from this
> changelog: the #87 upgrade quorum (`set_upgrade_quorum`, `cosigners`,
> `uqset` / `uprop`), #89 namespaced storage keys and `SCHEMA_VERSION = 2`,
> #90 genesis hash (3-argument `initialize`, `get_previous_wasm_hash`),
> `*_with_nonce` entry points, `get_transaction_history`, transaction expiry,
> guardian / auto-pause / attestation / renounce. `initialize` and `upgrade`
> keep their 2-argument signatures and the schema version stays `1`.

### Event schema

- Restored `EventBatchProcessed` (topic `batch`) to `src/events.rs`; it was
  catalogued as Live in `EVENTS.md` but its struct and emitter were lost in
  #197. Now also covered by the conformance manifest.
- `EventDisputeRaised` / `EventDisputeResolved` are now **Live**, emitted by
  `dispute_transaction` / `resolve_dispute`. An upheld `resolve_dispute`
  emits `status` (`Completed → Failed`) before `dsprslvd`.
- Added `cancel`, `retry`, `batch` (`EventBatchProcessed`), `partial`,
  `tagged`, `merged`, `fwd`, `ceiling`, `rs_prop`, `rs_canc`, `rs_add`,
  `rs_rm` and `rs_thr` events. Additive new events — Minor bump.
- `EventStatusChanged` can now carry `Cancelled` as `new_status`, and
  `Failed → Pending` (retry) as a transition. `TransactionStatus` gains a
  trailing `Cancelled` variant.
- EVENTS.md §2 now lists every emitted topic and is checked against
  `src/events.rs` by `event_decoder_covers_catalogued_topics`. Wave 2 events
  (`param`, `bonded`, `unbondrq`, `unbondcl`, `slashed`, `tierset`,
  `rebate`) were emitted but undocumented; they are now catalogued.

- Added `EventDisputeRaised` (topic `dispute`), emitted by the future
  `dispute_transaction` entry-point (sibling issue). Schema locked in
  `src/events.rs` and catalogued in `EVENTS.md`. Additive new event per
  [`EVENTS.md` § Semver policy](./EVENTS.md#5-semver-policy) — Minor bump.
- Added `EventDisputeResolved` (topic `dsprslvd`), emitted by the future
  `resolve_dispute` entry-point (sibling issue). Schema locked. The `upheld`
  boolean is normatively documented: `true` = dispute upheld, transaction
  reverted to `Failed`; `false` = dispute rejected, transaction returned to
  `Completed`. Additive new event — Minor bump.
- Added `EventUpgradeProposed` (`up_prop`), `EventUpgradeFinalized`
  (`up_fin`), `EventUpgradeCancelled` (`up_can`), `EventUpgradeRolledBack`
  (`rollback`), `EventUpgradeMigrated` (`migrate`) — additive new events
  (Minor) for issues #81–#83.
- Added `EventUpgradeSelfCheckPassed` (topic `chk_pass`) and
  `EventUpgradeSelfCheckFailed` (topic `chk_fail`), emitted by `upgrade`
  around the post-upgrade storage-integrity self-check. Additive new
  events — Minor bump.
- Added `EventFeeAccrued` (`fee`), `EventWithdrawalProposed` (`wprop`), and
  `EventWithdrawalExecuted` (`wexec`). Additive new events — Minor bump.
  `fee` is emitted by `complete_transaction` **after** `done`, only when a
  non-zero fee accrues; the existing `status` → `done` order is unchanged.

### Added

- On-chain fee accrual (#141): `complete_transaction` accrues
  `floor(amount * base_fee_bps / 10_000)` to `treasury_balance()`, reading
  the existing `base_fee_bps` registry param (unset = no fee). The math is
  overflow-free and checked (`ArithmeticOverflow`). See
  [ADR-0008](./docs/adr/0008-fee-accrual-and-treasury-withdrawal.md).
- Two-party treasury withdrawal (#142): admin `propose_withdrawal(amount,
  destination)`, then relay-signer `authorize_withdrawal(caller, amount,
  destination)`, which must restate the proposal
  (`WithdrawalProposalMismatch` otherwise). Withdrawals are capped per epoch
  by the `treasury_epoch_cap` / `treasury_epoch_length` params. Queries:
  `treasury_config()`, `pending_withdrawal()`. New error codes 110–117.
- `set_param` range-checks `base_fee_bps` (0–10 000), `treasury_epoch_cap`
  (> 0), and `treasury_epoch_length` (1–`u32::MAX`), rejecting other values
  with `InvalidParamValue`. Other param names are unchanged.
- `src/test_accepted_risks.rs` (#139): executable tests for the THREAT_MODEL.md
  §8 compensating controls, R-01 … R-06 (R-06 is new: treasury two-party auth).
- `tools/export-test-vectors` and `test-vectors.json` (#140): portable test
  vectors for independent auditor replay, documented in
  [`docs/test-vectors.md`](./docs/test-vectors.md).
- `get_dispute_queue(cursor, limit)` (#166): paginated, oldest-first list of
  open disputes, with stable sequence-number cursors. Backed by the restored
  `dispute_transaction` / `resolve_dispute` / `is_disputed` entry points and
  the new `get_dispute` query. At most `MAX_OPEN_DISPUTES` (100) may be open.
- Global `global_max_amount` ceiling (#169), set via the param registry and
  on by default (`DEFAULT_GLOBAL_MAX_AMOUNT` = 10^15). Enforced on single and
  batch registration alongside the restored per-anchor ceiling
  (`set_anchor_amount_ceiling`); the lower of the two wins. New queries
  `get_amount_ceiling(anchor)` and `get_global_max_amount()`.
- Resource-budget check in `batch_register_callback` (#173): a conservative
  write/event byte estimate rejects over-budget batches with
  `BatchBudgetExceeded` before any write. See `COST_MODEL.md` §11.1.
- Stricter Clippy configuration (#175): `pedantic`, `nursery` and `cargo`
  enabled in `Cargo.toml`, with individually justified exceptions.
  See `CONTRIBUTING.md` § Lints.
- Transaction lifecycle (#176–#178): `cancel_transaction` (terminal
  `Cancelled`), `retry_transaction` (`Failed → Pending`, at most
  `MAX_RETRIES` = 3), `batch_register_callback` (atomic, ≤ 20 payloads),
  paginated `get_transactions_by_status`, `partial_complete_transaction`,
  `add_transaction_tag` / `get_transaction_tags`, and per-anchor / default
  amount ceilings (`set_amount_ceiling`, `set_default_amount_ceiling`,
  `get_amount_ceiling`) enforced on every ingestion.
- Recovery and relay signers (#179): `merge_duplicate_transactions`,
  `set_forwarding_route` / `get_forwarding_route`, an N-of-M relay signer set
  (`relay_signer_set`, `add_relay_signer`, `remove_relay_signer`,
  `set_relay_threshold`, `approve_relay_call`) and timelocked relay rotation
  (`set_relay_signer_delay`, `propose_relay_signer`, `finalize_relay_signer`,
  `cancel_relay_signer_change`, `pending_relay_signer`).

### Fixed

- `main` compiles again: repaired merge damage in `src/events.rs` from #197
  and restored the batch-registration, dispute and per-anchor-ceiling code that
  `src/validation.rs` and the test suite still depended on. Tests for other
  entry points removed in #197 are quarantined; see `QUARANTINE.md`.
- `unbond_collateral` no longer truncates an out-of-range
  `unbond_delay_ledgers` param (e.g. `2^32 + 5` became a 5-ledger delay). A
  negative or oversized delay now fails closed (never claimable).
- `batch_register_callback` now rejects replayed or in-batch duplicate
  idempotency keys with `DuplicateRequest`.

- Timelocked upgrade flow (#81 / ADR-0004): `propose_upgrade`,
  `finalize_upgrade`, `cancel_upgrade`, `get_pending_upgrade`,
  `set_upgrade_delay` / `upgrade_delay`. Second propose **replaces** and
  restarts the delay.
- `upgrade_and_migrate` + versioned migration registry (#82 / ADR-0005)
  with `MAX_MIGRATION_STORAGE_TOUCHES` bound; oversized/resumable
  migrations documented as out of scope for v1.
- `rollback_upgrade` single-step previous-WASM restore (#83); blocked when
  the last upgrade used `upgrade_and_migrate` (`UpgradeNotReversible`).
  `register_installed_wasm` seeds the history slot post-deploy.
- Schema compatibility ranges (#84 / ADR-0006):
  `set_schema_compatibility_range` / `schema_compatibility_range`; default
  unset behaviour remains exact-match (ADR-0003).
- Resource-usage regression gate (#119): CI meters every hot entry point of
  the release WASM against `resource_baseline.toml` and fails on a >15%
  regression. See COST_MODEL.md §12 for the baseline-update process.
- Release WASM size gate (#122): CI fails if `make wasm`'s output grows >5%
  over `wasm_size.toml`'s baseline or exceeds a 96 KiB ceiling (75% of
  Soroban's 131 072-byte `contract_max_size_bytes`). See COST_MODEL.md §13.
- Test infrastructure (#131–#134):
  - Differential testing between the `release` and `release-with-logs`
    profiles (#131): `scripts/diff_profiles.sh` / `make profile-diff` diffs a
    return-value + event + ledger-state trace of every entry point across
    both profiles. It runs in the new `profile-diff` CI job alongside a
    self-test that proves it catches an injected `debug_assertions`
    divergence.
  - Relay-signer rotation × in-flight transaction harness (#132): exhaustive
    rotation-timing/kind/driver enumeration plus seeded chaos runs against a
    reference model, asserting stale signers are always rejected and no
    transaction is ever left stuck (`src/test_rotation_chaos.rs`).
  - Event-payload snapshot gate (#133): committed fixtures in
    `fixtures/event_snapshots/` pin exact topics and payload XDR for every
    emitting entry point and ordering variant. Drift fails `cargo test` with a
    diff, and updates need `SYNAPSE_UPDATE_EVENT_SNAPSHOTS=1`. Catalogue gaps
    against EVENTS.md §2 are tracked explicitly.
  - Boundary-value coverage for every numeric/length cap (#134), with the cap
    inventory and findings in `src/test_boundaries.rs`.

### Changed

- `register_callback` validation runs cheapest-first (#120); with several
  invalid fields the *first* error reported can differ (e.g. `InvalidAmount`
  before `InvalidStellarAccount`). Accept/reject outcomes are unchanged.
  Worst-case rejection −28% CPU; cheap rejections −45…−66%.
- Hot entry points allocate fewer host objects (#121): `start_processing`,
  `complete_transaction`, `fail_transaction` ≈ −18% CPU / −7% memory;
  `register_callback` −9% / −3%. No storage-layout or AB

### Changed

- `register_callback` validation runs cheapest-first (#120); with several
  invalid fields the *first* error reported can differ (e.g. `InvalidAmount`
  before `InvalidStellarAccount`). Accept/reject outcomes are unchanged.
  Worst-case rejection −28% CPU; cheap rejections −45…−66%.
- Hot entry points allocate fewer host objects (#121): `start_processing`,
  `complete_transaction`, `fail_transaction` ≈ −18% CPU / −7% memory;
  `register_callback` −9% / −3%. No storage-layout or ABI change.
- THREAT_MODEL.md §8 **R-05** status updated from accepted (no timelock) to
  **mitigated** via the propose/finalize flow.
- `upgrade()` schema guard now checks the configured `[min, max]` range
  instead of exact equality only (range defaults to exact match).
- **Breaking (error codes):** Soroban caps a contract error enum at 50
  variants, so related errors now share a code: `TimelockNotElapsed` (62)
  covers the upgrade, relay-signer and unbond delays (was
  `UnbondDelayNotElapsed` = 83); `NoPendingChange` (63) covers pending
  upgrades and relay-signer changes; `InvalidSlashEvidence` (90) replaces
  `EvidenceTxIdMismatch` / `EvidenceNotConflicting` (90 / 91). Unused
  `StorageError` (50) and `InvalidParamValue` (71) are removed.
- `set_relay_signer` returns `TimelockRequired` once a non-zero relay-signer
  delay is configured.
- **Breaking:** `MAX_BATCH_SIZE` is 7, not 20. Protocol 22 allows 25 ledger
  writes per transaction and each payload needs 3, so larger batches could
  never succeed on a real network. Batches are also checked against a
  conservative write/event budget before any write and rejected with the new
  `BatchResourceBudgetExceeded` (38) (#173).
- Cheaper hot path (#116, #117, #123): `register_callback` fee −14.7 %,
  batches −18 %, transitions −2 to −5 % (COST_MODEL.md §12). Transactions are
  stored as a packed `StoredTransaction`; `get_transaction` still returns
  `Transaction`. The relay signer set moved to instance storage.
- Ledger read/write budgets per entry point are pinned by
  `bench_resource_budgets` (#116, #123, #125).

### Fixed

- `unbond_collateral` converted the `unbond_delay_ledgers` param with a
  wrapping `as u32` cast, so a delay of 2^32 (or any multiple) became a zero
  delay and the unbond was claimable immediately, bypassing the slash window.
  Negative values wrapped to huge delays. The value is now clamped to
  `[0, u32::MAX]` before conversion (found by the #134 boundary audit).

### Event schema (prior unreleased)
- Added `EventRelaySignerRotated` (topic `relay`), emitted by
  `set_relay_signer`. Additive new event per
  [`EVENTS.md` § Semver policy](./EVENTS.md#5-semver-policy) — Minor bump.
- Added `EventAdminTransferProposed` (topic `propose`), emitted by the new
  `propose_admin`. Additive new event — Minor bump.
- **Breaking:** `EventAdminTransferred` (topic `admin`) is now emitted by
  `accept_admin` instead of the removed `transfer_admin`. The event's own
  topic/fields/order are unchanged, but which entry-point triggers it — and
  how many calls that takes — has changed; per
  [`EVENTS.md` § Semver policy](./EVENTS.md#5-semver-policy) this is a
  **Major** change. Subscriber teams: expect `admin` no longer immediately
  after a single admin-initiated call — it now follows a separate
  `accept_admin` call from the nominee, which may arrive in a later ledger
  or not at all if never accepted.
- `EventContractUpgraded` gains an additive trailing `schema_version` field
  — Minor bump per the same policy.
- `rollback_upgrade` and `upgrade_and_migrate` also emit `chk_pass` before
  `upgrade`, like every upgrade path.
- **Emission-order change for `upgrade`:** success now emits `chk_pass`
  then `upgrade` (was `upgrade` alone). Self-check failure emits `chk_fail`
  and reverts. Minor bump for the additive events; integrators that assumed
  a single `upgrade` event per successful call should tolerate the leading
  `chk_pass`.

### Added

- `simulate_upgrade(caller, new_wasm_hash, expected_schema_version) ->
  UpgradeCompatibility` — read-only dry-run of every guard `upgrade()`
  enforces, with distinguishable verdicts (`Compatible`, `NotInitialised`,
  `CallerNotAdmin`, `SchemaVersionMismatch`). Side-effect free.
- `get_upgrade_history() -> Vec<UpgradeRecord>` — bounded on-chain upgrade
  audit log (previous/new WASM hash, schema version, ledger, admin). Cap
  `MAX_UPGRADE_HISTORY` (32) with FIFO eviction. Pre-feature upgrades are
  not backfilled.
- Mandatory `post_upgrade_self_check` at the end of `upgrade()`; failure
  returns `SelfCheckFailed` and reverts the whole upgrade transaction.
- Explicit, tested single-call guarantee for `initialize()` (issue #79) —
  documentation of why caller auth is intentionally absent given Soroban's
  deployment model; residual pre-init front-running accepted operationally.
- `EventRelaySignerRotated` — relay-signer rotation is now observable
  on-chain the same way admin transfer already is (see
  [`EVENTS.md`](./EVENTS.md#eventrelaysignerrotated)).
- `admin()` / `relay_signer()` read-only query entry points. Every other
  piece of contract state readable off-chain already had a query method;
  these two let deployment tooling and monitoring verify the on-chain role
  addresses against `contract-ids.json` instead of trusting that record
  alone. See `DEPLOYMENT.md`'s post-deployment smoke test.
- `schema_version()` / `pending_admin()` read-only query entry points.
- **#88** `migrations.toml` + CI/`make schema-check` that builds the release
  WASM once, invokes `schema_version()`, and fails on exact-match miss
  (negative fixture under `fixtures/ci/`).

### Changed

- **Breaking:** `transfer_admin(new_admin)` is replaced by
  `propose_admin(new_admin)` + `accept_admin(caller)`. A single call from
  the current admin can no longer finalise a transfer on its own — the
  nominee must call `accept_admin` itself, proving key control via its own
  auth. Fixes THREAT_MODEL.md finding F-03. Integrators calling
  `transfer_admin` directly (not through this repo's SDK client) must
  switch to the two-step flow.
- **Breaking:** `upgrade(new_wasm_hash)` is now
  `upgrade(new_wasm_hash, expected_schema_version)`. The extra argument must
  match the on-chain `schema_version()` or the call is rejected with
  `SchemaVersionMismatch` before contract WASM is touched. Fixes
  THREAT_MODEL.md finding F-04.

### Fixed

- `main` builds and tests again: bad-merge fragments in `events.rs`, orphan
  validators referencing never-merged error variants, and test modules that
  did not compile. Tests for entry points lost in the #176–#197 merges are
  gated behind `cfg(synapse_quarantine)` (see `src/lib.rs`) until restored.
  `schema_ci` and `bench_events` are now compiled, so the #88 schema gate and
  event benches actually run; CI installs the `wasm32` target again.
- **Security:** `propose_admin` rejects nominating the contract's own
  address, which could not practically call `accept_admin` back and would
  have permanently bricked every admin-gated operation. Fixes
  THREAT_MODEL.md finding F-02 (Medium severity). Soroban has no "zero
  address" sentinel to check a nominee against generally — the contract's
  own address is the only "invalid address" this can detect on-chain.

- **Security:** `register_callback` no longer overwrites an existing
  transaction record on a late replay. Previously, once the ~24h idempotency
  key TTL expired, a replay with a different `idempotency_key` but the same
  `transaction_id` would silently reset a `Completed`/`Failed` transaction
  back to `Pending` and wipe its `stellar_tx_hash`/`failure_reason`. Fixes
  THREAT_MODEL.md finding F-07 (High severity). `register_callback` now
  returns `DuplicateRequest` for any `transaction_id` that already has a
  stored record, independent of idempotency-key state. Callers that relied
  on the old (buggy) re-registration behavior will now get an error instead.
- `complete_transaction` / `fail_transaction` now enforce the `stellar_tx_hash`
  (72 B) and `failure_reason` (64 B) length caps documented in
  [`COST_MODEL.md` §6](./COST_MODEL.md#6-string-length-cap-impact-enforced).
  The validators existed but were never wired into the handlers, so an
  oversized value from a compromised or buggy relay could bypass the
  documented rent-cost protection.
- `EVENTS.md` listed `EventStatusChanged`, `EventTransactionCompleted`,
  `EventTransactionFailed`, and `EventAdminTransferred` as "Locked schema
  (emitter scaffold)". All four have been wired into `src/lib.rs` and covered
  by tests since before this release; the doc now says **Live** to match.
  Document-only correction — no behaviour change, no version bump per
  [`EVENTS.md` § Semver policy](./EVENTS.md#5-semver-policy).

---

## [0.1.0] — 2026-07-22

### Event schema

Initial lock of the public event API (see [`EVENTS.md`](./EVENTS.md)).

| Topic[1] | Struct | Entry-point(s) | Notes |
|----------|--------|----------------|--------|
| `init` | `EventInitialised` | `initialize` | Live |
| `reg` | `EventTransactionRegistered` | `register_callback` | Live; omitted on idempotent replay |
| `pause` | `EventPauseToggled` | `pause`, `unpause` | Live |
| `upgrade` | `EventContractUpgraded` | `upgrade` | Live |
| `status` | `EventStatusChanged` | lifecycle transitions | Schema locked; emitter scaffold |
| `done` | `EventTransactionCompleted` | `complete_transaction` | Schema locked; after `status` |
| `fail` | `EventTransactionFailed` | `fail_transaction` | Schema locked; after `status` |
| `admin` | `EventAdminTransferred` | `transfer_admin` | Schema locked; emitter scaffold |

Guaranteed order on `complete_transaction`: `status` then `done`.  
Guaranteed order on `fail_transaction`: `status` then `fail`.

Semver policy for future schema edits: [`EVENTS.md` § Semver policy](./EVENTS.md#5-semver-policy).

### Added

- On-chain transaction registry scaffold (Phase 1).
- Live emitters: `init`, `reg`, `pause`, `upgrade`.
- Admin-gated `upgrade`, pause circuit breaker, validation and storage helpers.

---

## Event-schema entry format (for maintainers)

When revising the event API, add a bullet under **Event schema** using this
shape so subscriber teams can scan without reading the full code diff:

```markdown
### Event schema

- **BREAKING (major):** rename topic `reg` → `register` — Phase 2/3 notice: YYYY-MM-DD.
- **Additive (minor):** append field `memo: String` to `EventTransactionRegistered`.
- **Docs (patch):** clarify that idempotent `register_callback` emits no events.
```

Rules of thumb (normative text in [`EVENTS.md`](./EVENTS.md#5-semver-policy)):

- Removal / rename / reorder / type change / emission-order change → **major** + advance notice.
- New trailing field or new event type → **minor** (or patch if docs-only).
