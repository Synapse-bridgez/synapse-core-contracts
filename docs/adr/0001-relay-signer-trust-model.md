# ADR-0001 — Relay-signer trust model

> **Status:** Accepted
> **Date:** 2025-Q1
> **Deciders:** Synapse Bridge relay operator, contract author

---

## Context

The Synapse Bridge Phase 1 contract acts as an **on-chain registry** for fiat
gateway transactions. It must accept new transaction records from an off-chain
service (`synapse-core`) without being callable by arbitrary Stellar accounts.

The contract needs to answer one fundamental question at the access-control
boundary:

> **Who is authorised to call `register_callback()`, and how does the contract
> verify that identity?**

The Anchor Platform (SEP-6/SEP-24) — which drives the fiat side of the bridge —
cannot sign Stellar transactions directly. It emits callback events, and the
off-chain `synapse-core` relay service consumes those events and submits the
corresponding on-chain calls.

Two further constraints shaped the design space:

1. The contract must not allow `register_callback` to be called by anyone — that
   would let any party inject fraudulent transaction records.
2. The relay service is a single logical actor; using a multi-key scheme would
   add operational complexity with no corresponding security benefit at this
   phase.

---

## Options considered

### Option A — Whitelist a single `relay_signer` address ✅ (chosen)

Store one `relay_signer: Address` in persistent storage at initialisation.
`register_callback()` calls `relay_signer.require_auth()` before doing any
work. The relay service holds the corresponding private key and signs every
submission.

**Pros:**
- Simple to reason about: exactly one key can register callbacks.
- `require_auth()` is a native Soroban primitive — no custom signature
  verification code to audit or get wrong.
- The signer key is operationally separate from the admin key (separation of
  privilege).
- The `relay_signer` address can be rotated by the admin without a contract
  upgrade.

**Cons:**
- A single key is a single point of compromise. If the relay service's signing
  key is leaked, an attacker can inject arbitrary transaction records until the
  admin rotates the key.
- The relay service must keep the private key available at runtime (hot key),
  which is a higher security requirement than, say, a cold multisig admin.

**Mitigations:**
- The relay key should live in a secrets manager (e.g. AWS Secrets Manager or
  HashiCorp Vault), not in a config file.
- The admin can rotate `relay_signer` at any time via `set_relay_signer()`.
- Off-chain monitoring should alert on unexpected `relay_signer` rotation events.

### Option B — Use the admin key for relay submissions

Have the same admin address sign both admin operations and `register_callback`
calls.

**Pros:**
- No second key to manage.

**Cons:**
- Conflates two different trust roles with very different operational profiles:
  admin operations are rare and should use a cold multisig; relay submissions
  happen continuously and require a hot key.
- Compromise of the relay key would also compromise admin capability (pause,
  rotate, upgrade).
- Violates the principle of least privilege.

### Option C — Open `register_callback` to any caller, validate on payload

Allow any Stellar account to call `register_callback`, but validate the payload
signature (e.g. an HMAC or Ed25519 signature over the callback body embedded in
the arguments).

**Pros:**
- Decouples the Stellar signing key from the authorisation decision.

**Cons:**
- Requires custom cryptographic verification inside the contract — more code,
  more audit surface, higher chance of a subtle bug.
- The contract would need to store and verify a public key for payload
  signature verification anyway, which is equivalent in trust to Option A but
  more complex.
- Soroban's `require_auth()` is already the idiomatic, well-audited primitive
  for "this address must have authorised this call".

---

## Decision

**Use Option A: a single `relay_signer` address stored in persistent storage,
verified via `require_auth()` in `register_callback()`.**

The relay service holds the corresponding private key as a hot key in a secrets
manager. The admin key is kept cold (multisig) and used only for infrequent
admin operations.

This is the simplest correct design. It separates the relay role from the admin
role, uses Soroban's native auth primitive, and allows key rotation without a
contract upgrade.

---

## Consequences

**Positive:**
- `register_callback()` has a clear, auditable access-control check in a single
  line: `relay_signer.require_auth()`.
- Relay key rotation is a live admin operation — no downtime, no contract
  upgrade.
- The separation between `relay_signer` (hot, continuous) and `admin`
  (cold, rare) limits the blast radius of either key being compromised.

**Negative / accepted trade-offs:**
- The relay signing key must be available at runtime. It is a hot key with a
  meaningful blast radius (fraudulent transaction injection) if leaked.
- A single `relay_signer` is a single point of failure. If the key is lost
  before rotation, the admin must rotate to a new key to restore operation.

**Follow-up work:**
- Phase 2 may introduce multi-relay support if horizontal scaling of the relay
  service requires more than one signing identity. That would change this
  decision and should produce a new ADR superseding this one.
- Key-rotation runbooks should be documented in `DEPLOYMENT.md` before mainnet.

---

## References

- [`src/admin.rs`](../../src/admin.rs) — `relay_signer` storage and rotation logic
- [`src/lib.rs`](../../src/lib.rs) — `register_callback()` entry-point with `require_auth()` call
- [`README.md`](../../README.md) — "Why relay_signer instead of direct Anchor Platform calls?"
- [`DECISIONS.md`](../../DECISIONS.md) — §6 Trust Assumptions (admin key requirements)
- [Soroban Auth docs](https://developers.stellar.org/docs/smart-contracts/example-contracts/auth)
