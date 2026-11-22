# Test vectors for independent replay

[`test-vectors.json`](../test-vectors.json) is a curated set of contract
scenarios in a tooling-agnostic JSON format. It lets an external auditor
check the deployed contract's behaviour with **their own** tooling, without
running or trusting this repository's Rust test suite.

The set is deliberately small: 38 vectors that cover the core happy paths,
auth boundaries, state-machine edges, and the fee and treasury accounting
paths. It is not a dump of every test.

---

## Regenerating

The vectors are defined in
[`tools/export-test-vectors/src/main.rs`](../tools/export-test-vectors/src/main.rs).
That crate is standalone and is not part of the contract build.
It serializes the curated expectations below; it does not execute the contract
or verify those expectations against the current implementation. Validate
behavior with the contract test suite and review vector changes when behavior
changes.

```bash
cd tools/export-test-vectors
cargo run -- --pretty > ../../test-vectors.json
```

Output is deterministic because all maps are key-sorted. Commit the
regenerated file together with any change to the tool, and review the diff
the same way as code.

---

## File format (`schema_version: 1`)

Top level:

| Field | Type | Meaning |
|-------|------|---------|
| `schema_version` | number | Version of **this JSON format**. Incremented on any breaking change to the fields below. Currently `1`. |
| `contract_version` | string | Contract semver the vectors were written against (e.g. `"0.1.0"`). |
| `description` | string | Free-text summary of the vector set. |
| `reference_docs` | string[] | Repo-relative documents that define the expected behaviour. |
| `vectors` | Vector[] | The scenarios, in declaration order. |

Each vector (`TestVector` in the exporter):

| Field | Type | Meaning |
|-------|------|---------|
| `id` | string | Stable identifier of the form `<area>-NNN` (e.g. `"init-001"`). IDs are never reused. |
| `description` | string | One-line human-readable intent of the scenario. |
| `category` | string | Functional area: `"initialisation"`, `"lifecycle"`, `"validation"`, `"auth"`, `"idempotency"`, `"fee"`, `"treasury"`, `"pause"`, `"admin"`, or `"upgrade"`. |
| `entry_point` | string | Contract function to invoke (e.g. `"register_callback"`). |
| `inputs` | object | Argument name → value map, keyed by the contract's parameter names. Keys are sorted alphabetically in the output. |
| `pre_state` | object | Conditions that must hold **before** the call (see below). Keys are sorted alphabetically. |
| `expected_result` | string | `"ok"` for success; `"ok:\"<value>\""` when the entry point returns a value (e.g. `"ok:\"tx-uuid-001\""`); `"err:<ContractError variant>"` for a contract-level error; or `"err:auth_error"` for a host-level authorisation failure. |
| `expected_events` | object[] | Each element is `{ "topics": ["synapse", "<name>"], "present": <bool> }`. `present: true` asserts the event is emitted; `present: false` asserts it is **not** emitted. |
| `expected_state` | object | Conditions that must hold **after** the call. Empty object `{}` means no post-call state assertions. Keys are sorted alphabetically. |
| `notes` | string[] | Cross-references to THREAT_MODEL.md finding IDs (F-xx, R-xx, I-xx), issue numbers, and numeric error codes. |

### Value encoding

- **`i128` values are decimal strings** (e.g. `"1000000"`), because JSON
  numbers cannot represent `i128` exactly. `u32` and `bool` values are native
  JSON numbers/booleans. `null` JSON values mean the field or entry is absent.
- **`<placeholder>` strings** such as `<admin_address>`, `<relay_address>`, and
  `<destination_address>` stand for addresses you generate. The same
  placeholder within one vector means the same address.
- **`caller_is_admin` / `caller_is_relay`** in `pre_state` indicate whose
  signature authorises the call.
- **Dotted keys** (`transaction.status`, `pending_withdrawal.amount`) address
  a field of a stored struct, readable through the matching query
  (`get_transaction`, `pending_withdrawal`, …). `param.<name>` is the value
  of a param-registry entry (`get_param(<name>).value`), and `null` means
  the param is unset.
- A **string condition** such as `">= epoch_start + treasury_epoch_length"` for
  `ledger_sequence` describes a ledger position to reach before the call.

Error variants map to numeric codes in `src/types.rs` `ContractError`, and
the relevant code is repeated in `notes` (for example
`ContractError::WithdrawalProposalMismatch = 117`). Event payload fields are
specified in [`EVENTS.md`](../EVENTS.md).

---

## Worked example

The following is `init-001`, copied verbatim from the current
[`test-vectors.json`](../test-vectors.json):

```json
{
  "id": "init-001",
  "description": "initialize() with valid admin and relay — succeeds and sets state",
  "category": "initialisation",
  "entry_point": "initialize",
  "inputs": {
    "admin": "<admin_address>",
    "relay_signer": "<relay_address>"
  },
  "pre_state": {
    "initialized": false
  },
  "expected_result": "ok",
  "expected_events": [
    {
      "topics": [
        "synapse",
        "init"
      ],
      "present": true
    }
  ],
  "expected_state": {
    "admin": "<admin_address>",
    "initialized": true,
    "paused": false,
    "relay_signer": "<relay_address>",
    "schema_version": 1
  },
  "notes": [
    "THREAT_MODEL §4.1: double-init guard enforced",
    "I-08: contract must be initialised before state-mutating calls"
  ]
}
```

**Reading this vector:**

1. `pre_state` requires the contract to not yet be initialised (`"initialized": false`).
2. Call `initialize` with an `admin` address and a `relay_signer` address (both generated by you).
3. The call must return `"ok"` (no return value).
4. Exactly one event with topics `["synapse", "init"]` must be emitted.
5. After the call, all five `expected_state` keys must match: `initialized` is now `true`,
   `admin` and `relay_signer` hold the addresses you supplied, `paused` is `false`,
   and `schema_version` is `1`.

---

## How an auditor replays a vector

1. Deploy the audited WASM to a sandbox or testnet. Check its SHA-256 hash
   against the one listed in THREAT_MODEL.md §10.5.
2. Establish `pre_state` using only public entry points: `initialize`,
   `register_callback`, `start_processing`, `complete_transaction` (to seed the
   treasury), `propose_withdrawal`, and `set_param` (for `param.*` keys). Advance the ledger
   where the vector asks for it.
3. Invoke `entry_point` with `inputs`, signed by the party that `pre_state`
   names.
4. Check that the result matches `expected_result`, that each
   `expected_events` entry is present or absent among this invocation's events
   (order per EVENTS.md §4), and that each `expected_state` key matches after a
   read through the public queries.

A failed call is rolled back as a whole. Where a vector expects an error,
`expected_state` records the values that must be **unchanged**.
