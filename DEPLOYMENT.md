# Deployment Guide

> **Audience:** Relay operator / contract deployer for the Synapse Bridge ecosystem.

---

## Prerequisites

- Rust 1.81+ with `wasm32-unknown-unknown` target
- `stellar-cli` ≥ 22.x
- Admin key(s) held by a **multisig or DAO** (see [DECISIONS.md](./DECISIONS.md) for rationale)

```bash
rustup target add wasm32-unknown-unknown
cargo install --locked stellar-cli --features opt
```

---

## Build

```bash
cargo build --target wasm32-unknown-unknown --release
```

The compiled WASM is at:
```
target/wasm32-unknown-unknown/release/synapse_core_contract.wasm
```

---

## Deploy (testnet / mainnet)

```bash
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/synapse_core_contract.wasm \
  --source <DEPLOYER_SECRET> \
  --network testnet
```

> **Security note:** The deployer key is **not** the admin key.  After
> deployment, the admin key (held by a multisig) must call `initialize()`.
> Never reuse the deployer secret for any other role.

---

## Initialise

```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ADMIN_SECRET> \
  --network testnet \
  -- \
  initialize \
  --admin <ADMIN_PUBLIC_KEY> \
  --relay_signer <RELAY_SIGNER_PUBLIC_KEY>
```

This is a one-time call; it will fail with `AlreadyInitialised` if called
again.

---

## Post-Deployment Checklist

- [ ] Admin key is a **multisig** (≥3-of-5 recommended, see [DECISIONS.md](./DECISIONS.md))
- [ ] Relay signer key is held by the off-chain relay service, **separate** from the admin
- [ ] Off-chain monitoring subscribes to the `(synapse, upgrade)` event to detect unexpected upgrades
- [ ] `stellar_tx_hash` field is capped at 72 B in [validation.rs](./src/validation.rs) 
      (otherwise storage cost is unbounded — see [COST_MODEL.md](./COST_MODEL.md))

---

## Upgrade (if needed)

If a bug fix or state-machine extension is required:

```bash
# 1. Build the new WASM
cargo build --target wasm32-unknown-unknown --release

# 2. Upload the WASM and get its hash
stellar contract upload \
  --wasm target/wasm32-unknown-unknown/release/synapse_core_contract.wasm \
  --source <ADMIN_SECRET> \
  --network testnet

# 3. Invoke upgrade
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source <ADMIN_SECRET> \
  --network testnet \
  -- \
  upgrade \
  --new_wasm_hash <HASH_FROM_STEP_2>
```

---

## Cost Estimates

See **[COST_MODEL.md](./COST_MODEL.md)** for a full per-transaction XLM cost
breakdown, including:

- Persistent and temporary storage footprint
- Write, read, and TTL-extension fees
- Lifecycle totals (minimum and typical)
- Monthly budget estimates at various volumes
- Impact of string-length caps on worst-case costs

**TL;DR:** Approximately **0.016–0.027 XLM per transaction** at current
baseline fees, dominated by the initial persistent-write cost.

---

## Monitoring

| Metric | Source | Action |
|--------|--------|--------|
| `ContractUpgraded` event | Event subscription | Alert — unexpected upgrade may indicate key compromise |
| Balance of admin account | Horizon | Ensure sufficient XLM for rent + fees |
| Transaction volume | Off-chain relay logs | Re-budget if volume exceeds deployment estimate |
| String lengths | Contract validation | Ensure `validation.rs` caps are enforced on deploy |

---

## Admin Key Requirements

> **The admin key is the single most important secret in the Synapse Bridge
> ecosystem.**  See [DECISIONS.md](./DECISIONS.md) §6 for the full trust model.

- **MUST** be held by a multisig (minimum 3-of-5) or a DAO
- **SHOULD NOT** be the same key used for deployment or relay signing
- **SHOULD** be monitored for unexpected usage
- **SHOULD** have a key-rotation plan documented before mainnet launch

