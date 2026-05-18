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

```bash
rustup target add wasm32-unknown-unknown
cargo install --locked stellar-cli --features opt
```

### Build

```bash
cargo build --target wasm32-unknown-unknown --release
```

### Test

```bash
cargo test
```

### Deploy (testnet)

```bash
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/synapse_core_contract.wasm \
  --source <DEPLOYER_SECRET> \
  --network testnet
```

---

## Key design decisions

### Why relay_signer instead of direct Anchor Platform calls?

The Anchor Platform can't sign Stellar transactions directly; the off-chain  
`synapse-core` service acts as the authenticated relay. The contract trusts one  
specific `relay_signer` address whose key is held by the relay service.

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
