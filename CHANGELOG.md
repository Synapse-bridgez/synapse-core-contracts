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

- _(none)_

### Added

### Changed

### Fixed

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
