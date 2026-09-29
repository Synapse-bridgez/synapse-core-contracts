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

### Event schema

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

### Added

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

### Changed

- THREAT_MODEL.md §8 **R-05** status updated from accepted (no timelock) to
  **mitigated** via the propose/finalize flow.
- `upgrade()` schema guard now checks the configured `[min, max]` range
  instead of exact equality only (range defaults to exact match).

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
- **Emission-order change for `upgrade`:** success now emits `chk_pass`
  then `upgrade` (was `upgrade` alone). Self-check failure emits `chk_fail`
  and reverts. Minor bump for the additive events; integrators that assumed
  a single `upgrade` event per successful call should tolerate the leading
  `chk_pass`.
- Added `EventUpgradeQuorumSet` (topic `uqset`) and `EventUpgradeProposed`
  (topic `uprop`) for the optional upgrade quorum (#87) — Minor bump.

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
- **#87** Optional upgrade M-of-N quorum (`set_upgrade_quorum` /
  `upgrade_quorum` / `propose_upgrade` / `approve_upgrade`). Default `None`
  preserves single-admin upgrades; when set, admin-alone is rejected.
- **#89** Namespaced `StorageKey::Ns(STORAGE_KEY_NAMESPACE, DataKey)` plus
  `migrate_storage_keys` / `upgrade_and_migrate` for the v1 → v2 cutover.
- **#90** `get_previous_wasm_hash()` — self-reported hash the contract most
  recently upgraded from (genesis recorded at `initialize`).
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
- **Breaking:** `initialize(admin, relay_signer)` is now
  `initialize(admin, relay_signer, wasm_hash)` so genesis provenance is
  on-chain for `#90`.
- **Breaking:** `upgrade(…)` gains a trailing `cosigners: Vec<Address>`
  argument (empty when no upgrade quorum is configured) — `#87`.
- **Breaking:** on-chain `SCHEMA_VERSION` is now `2` (namespaced storage
  keys, `#89`).

### Fixed

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

## Unreleased

- Added `get_nonce` and `*_with_nonce` variants of privileged entry points
  (per-address strictly sequential nonce; gaps and reuse fail with `InvalidNonce`).
  Existing signatures are unchanged; off-chain relay should migrate to the
  nonce variants before the plain ones are removed in a future breaking release.
- Added append-only `get_transaction_history` (cap 32 entries, oldest evicted). Transactions registered before this release have no history; an off-chain migration note applies (no on-chain backfill).
