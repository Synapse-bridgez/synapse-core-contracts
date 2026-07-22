# synapse-core-contract

> **Soroban smart contract** — Phase 1 of the [Synapse Bridge](https://github.com/synapse-bridgez) ecosystem.

This contract is the **on-chain mirror** of the `synapse-core` off-chain relay service.  
It provides an auditable, idempotent transaction registry on Stellar and drives the  
three-phase lifecycle that ultimately bridges fiat deposits to cross-chain assets.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     Synapse Bridge Ecosystem                    │
│                                                                 │
│   Phase 1: Fiat Gateway          Phase 2         Phase 3        │
│   ┌──────────────┐          ┌───────────┐   ┌──────────────┐   │
│   │ synapse-core │  ──────► │   Swap    │──►│ Cross-Chain  │   │
│   │  (off-chain) │          │  Engine   │   │   Bridge     │   │
│   └──────┬───────┘          └───────────┘   └──────────────┘   │
│          │ relay_signer                                         │
│          ▼                                                      │
│   ┌──────────────────────────────┐                             │
│   │  synapse-core-contract       │  ◄── this repo             │
│   │  (on-chain registry)         │                             │
│   └──────────────────────────────┘                             │
└─────────────────────────────────────────────────────────────────┘
```

### Transaction lifecycle (on-chain)

```
Pending ──► Processing ──► Completed
        └──────────────► Failed
```

| Transition              | Caller            | Entry-point              |
|-------------------------|-------------------|--------------------------|
| `→ Pending`             | `relay_signer`    | `register_callback()`    |
| `Pending → Processing`  | relay or admin    | `start_processing()`     |
| `Processing → Completed`| relay or admin    | `complete_transaction()` |
| `* → Failed`            | relay or admin    | `fail_transaction()`     |

---

## Module layout

```
src/
├── lib.rs          ← contract entry-point, all public #[contractimpl] methods
├── types.rs        ← Transaction, TransactionStatus, CallbackPayload, StorageKey, ContractError
├── storage.rs      ← ledger read/write helpers (persistent / temporary / instance)
├── events.rs       ← typed event structs + EventEmitter
├── validation.rs   ← stateless input guards
├── admin.rs        ← role-based access control (admin + relay_signer)
└── tests.rs        ← integration test skeletons (one per entry-point)
```

---

## What is implemented (~10%)

| Component        | Status      | Notes                                              |
|------------------|-------------|----------------------------------------------------|
| `types.rs`       | ✅ Complete  | All structs, enums, error codes defined            |
| `lib.rs`         | 🔲 Scaffold | Function signatures + doc comments + `TODO` bodies |
| `storage.rs`     | 🔲 Scaffold | `is_initialised()` works; all writes are `todo!()` |
| `events.rs`      | 🔲 Scaffold | Event structs defined; emit calls are `todo!()`    |
| `validation.rs`  | 🔲 Scaffold | Function signatures + logic outline in comments    |
| `admin.rs`       | 🔲 Scaffold | Access-control logic outlined in `TODO` comments   |
| `tests.rs`       | 🔲 Scaffold | Full test matrix defined; bodies are `todo!()`     |

---

## Getting started

### Prerequisites

- Rust 1.81+ with `wasm32-unknown-unknown` target  
- `stellar-cli` ≥ 22.x
- `make` (pre-installed on macOS/Linux; on Windows use [WSL](https://learn.microsoft.com/en-us/windows/wsl/) or [GNU Make for Windows](https://gnuwin32.sourceforge.net/packages/make.htm))

```bash
rustup target add wasm32-unknown-unknown
cargo install --locked stellar-cli --features opt
```

### First-time setup

After cloning, run the one-time setup to install the pre-commit hook:

```bash
make setup
```

This registers `.git-hooks/pre-commit` so that every commit automatically
runs `cargo fmt --check` and `cargo clippy` before it is accepted locally —
the same checks CI enforces.

### Run the full check suite

```bash
make check
```

`make check` runs **fmt → clippy → test → wasm build** in order, using
exactly the same flags as the CI job. A green `make check` on your machine
means the same commit will pass CI.

| Target | Command it runs |
|---|---|
| `make fmt` | `cargo fmt --all -- --check` |
| `make clippy` | `cargo clippy --all-targets -- -D warnings -A clippy::todo` |
| `make test` | `cargo test --verbose` |
| `make wasm` | `cargo build --target wasm32-unknown-unknown --release` |
| `make build` | `cargo build --verbose` (quick debug build) |
| `make check` | all of the above, in order |

### Build

```bash
# Debug build
make build

# Release wasm artefact
make wasm
```

### Test

```bash
make test
# or directly:
cargo test
```

### Deploy

See **[DEPLOYMENT.md](./DEPLOYMENT.md)** for a full deployment guide covering
initialisation, upgrade, post-deployment checklist, admin key requirements, and
monitoring setup.

### Cost estimates

See **[COST_MODEL.md](./COST_MODEL.md)** for the per-transaction XLM cost
model, including persistent and temporary storage footprints, rent fees, and
monthly budget projections at various volumes.

---

## Contributing

See **[CONTRIBUTING.md](./CONTRIBUTING.md)** for branch/PR conventions, how to
run the local check suite, doc-comment expectations, and guidance on when to
write an Architecture Decision Record.

---

## Key design decisions

Non-obvious decisions are recorded as Architecture Decision Records in
[`docs/adr/`](./docs/adr/). The ADR log is the canonical place to understand
*why* a design choice was made, what alternatives were considered, and what
trade-offs were accepted.

### Why relay_signer instead of direct Anchor Platform calls?

The Anchor Platform can't sign Stellar transactions directly; the off-chain  
`synapse-core` service acts as the authenticated relay. The contract trusts one  
specific `relay_signer` address whose key is held by the relay service.  
See **[ADR-0001](./docs/adr/0001-relay-signer-trust-model.md)** for the full
trust-model analysis.

### Idempotency (on-chain)

Idempotency keys are stored in **temporary** ledger storage (~24 h TTL), mirroring  
the Redis-based deduplication in the off-chain service. Duplicate `register_callback`  
calls within the window return the original `tx_id` without re-writing.

### Storage tiers

| Data             | Tier        | Reason                                  |
|------------------|-------------|-----------------------------------------|
| Admin / relay    | `persistent`| Must survive contract instance restore  |
| Transactions     | `persistent`| Long-lived audit record                 |
| Idempotency keys | `temporary` | Self-expiring after 24 h (≈18 000 ledgers) |
| Init flag        | `instance`  | Lives with the contract instance        |

### In-Place Contract Upgradability

**Status:** ✅ Supported

This contract includes an `upgrade(new_wasm_hash)` entry point gated by the admin
role.  The full rationale, trade-off analysis, and upgrade-boundary guarantees
are documented in [`DECISIONS.md`](./DECISIONS.md).

**Key guarantee:** Persistent and instance storage survive a same-schema upgrade.
Only temporary storage (idempotency keys) is evicted — acceptable because their
TTL is short and the contract rejects duplicates via existing transaction records.

**Trust requirement:** The admin key **MUST** be held by a multisig (≥3-of-5) or
a DAO.  Admin-key compromise allows arbitrary WASM deployment, not just role
rotation.  See [`DECISIONS.md`](./DECISIONS.md#6-trust-assumptions) for the full
trust model.

**When to upgrade:**
- Bug fixes in contract logic
- Adding new read-only queries or event fields
- State-machine extensions for future phases

**When to deploy fresh:**
- Breaking changes to `Transaction` struct layout or `StorageKey` variants
- Fundamental access-control model changes
- WASM size exceeds Soroban deployment constraints
