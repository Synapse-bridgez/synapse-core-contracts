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
| `schema_version` | number | Version of **this JSON format**. Incremented on any breaking change to the fields below. |
| `contract_version` | string | Contract `version()` the vectors were written against. |
| `description` | string | Free-text summary. |
| `reference_docs` | string[] | Repo-relative documents that define the expected behaviour. |
| `vectors` | Vector[] | The scenarios. |

Each vector:

| Field | Type | Meaning |
|-------|------|---------|
| `id` | string | Stable identifier (`<area>-NNN`). IDs are never reused. |
| `description` | string | Intent of the scenario, in one line. |
| `category` | string | `initialisation`, `validation`, `auth`, `idempotency`, `lifecycle`, `fee`, `treasury`, `pause`, `admin`, `upgrade`. |
| `entry_point` | string | Contract function to invoke. |
| `inputs` | object | Argument name → value, using the contract's argument names. |
| `pre_state` | object | Conditions that must hold **before** the call (see below). |
| `expected_result` | string | `"ok"` for a successful call with no return value; `"ok:<json_value>"` for a successful call that returns a value (the return value is JSON-encoded and appended after the colon, e.g. `"ok:\"tx-uuid-001\""`); `"err:<ContractError variant>"` for a contract-level error; or `"err:auth_error"` for a host-level auth failure (not a `ContractError`). |
| `expected_events` | object[] | Each entry is `{ "topics": ["synapse", "<name>"], "present": bool }`. `present: false` asserts the event is **not** emitted. An empty array means no event assertions are made for this vector. |
| `expected_state` | object | Conditions that must hold **after** the call. An empty object means no post-state assertions. |
| `notes` | string[] | Cross-references to THREAT_MODEL.md IDs (F-xx, R-xx, I-xx), issues, and error codes. |

### Example

The following is an excerpt from `test-vectors.json` showing one lifecycle vector and one error vector:

```json
{
  "schema_version": 1,
  "contract_version": "0.1.0",
  "description": "Language-agnostic test vectors for synapse-core-contract. ...",
  "reference_docs": ["THREAT_MODEL.md", "EVENTS.md", "..."],
  "vectors": [
    {
      "id": "reg-001",
      "description": "register_callback() with valid payload — creates Pending transaction",
      "category": "lifecycle",
      "entry_point": "register_callback",
      "inputs": {
        "amount": "1000000",
        "anchor_transaction_id": "anchor-001",
        "asset_code": "USDC",
        "asset_issuer": "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ",
        "callback_status": "pending_external",
        "callback_type": "Deposit",
        "idempotency_key": "idem-001",
        "stellar_account": "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ",
        "transaction_id": "tx-uuid-001"
      },
      "pre_state": {
        "idempotency_known": false,
        "initialized": true,
        "paused": false,
        "transaction_exists": false
      },
      "expected_result": "ok:\"tx-uuid-001\"",
      "expected_events": [
        { "topics": ["synapse", "reg"], "present": true }
      ],
      "expected_state": {
        "idempotency_key_recorded": true,
        "transaction.failure_reason": "",
        "transaction.status": "Pending",
        "transaction.stellar_tx_hash": ""
      },
      "notes": [
        "I-01: transaction created in Pending state",
        "I-10: stellar_tx_hash empty until Completed",
        "I-11: failure_reason empty until Failed"
      ]
    },
    {
      "id": "init-002",
      "description": "initialize() called twice — returns AlreadyInitialised",
      "category": "initialisation",
      "entry_point": "initialize",
      "inputs": {
        "admin": "<admin_address>",
        "relay_signer": "<relay_address>"
      },
      "pre_state": { "initialized": true },
      "expected_result": "err:AlreadyInitialised",
      "expected_events": [
        { "topics": ["synapse", "init"], "present": false }
      ],
      "expected_state": {},
      "notes": ["ContractError::AlreadyInitialised = 1"]
    }
  ]
}
```

### Value encoding

- **`i128` values are decimal strings** (for example `"1000000"`), because JSON
  numbers cannot represent `i128` exactly. `u32` and `bool` values are native
  JSON.
- **`<placeholder>` strings** such as `<admin_address>`, `<relay_address>`, and
  `<destination_address>` stand for addresses you generate. The same
  placeholder within one vector means the same address.
- **`caller_is_admin` / `caller_is_relay`** in `pre_state` say whose
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
