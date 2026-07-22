# Contributing to synapse-core-contracts

Thanks for contributing to the Synapse Bridge smart contract. This document
covers the conventions the project uses so your changes land smoothly.

---

## Table of contents

- [Branch and PR conventions](#branch-and-pr-conventions)
- [Local checks](#local-checks)
- [Code style](#code-style)
- [Doc-comment expectations](#doc-comment-expectations)
- [Architecture Decision Records](#architecture-decision-records)
- [Issue tracker](#issue-tracker)

---

## Branch and PR conventions

### Branch naming

| Branch type         | Pattern                           | Example                                  |
|---------------------|-----------------------------------|------------------------------------------|
| Feature / new work  | `feat/<short-description>`        | `feat/pause-endpoint`                    |
| Bug fix             | `fix/<short-description>`         | `fix/idempotency-key-collision`          |
| Documentation       | `docs/<short-description>`        | `docs/adr-relay-signer`                  |
| Refactor            | `refactor/<short-description>`    | `refactor/storage-helpers`               |
| Chore / tooling     | `chore/<short-description>`       | `chore/update-stellar-cli-version`       |

- Branch off `main` for every change — there are no long-lived integration
  branches.
- Delete your branch after the PR is merged.

### Commit messages

Use the [Conventional Commits](https://www.conventionalcommits.org/) format:

```
<type>(<optional scope>): <short summary>

<optional body — wrap at 72 chars>

<optional footer, e.g. Closes #37>
```

Common types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`.

Keep the subject line under **72 characters** and in the imperative mood
("add pause endpoint", not "added" or "adds").

### Pull requests

- **Target branch:** `main`.
- **Title:** mirrors the commit subject (≤ 72 chars, imperative mood).
- **Description:** summarise *what* changed, *why*, and *what was tested*.
  Link the relevant issue with `Closes #<n>` so GitHub closes it on merge.
- Every PR must pass `make check` before review (CI enforces this).
- At least one approval is required before merge.
- Squash-merge is preferred to keep `main` history linear.

---

## Local checks

### One-time setup

```bash
make setup
```

This installs `.git-hooks/pre-commit`, which runs `cargo fmt --check` and
`cargo clippy` before every commit — the same gates CI enforces.

### Full check suite

```bash
make check
```

`make check` runs **fmt → clippy → test → wasm build** in order.  A green
`make check` locally means the same commit will pass CI.

| Target       | What it runs                                                    |
|--------------|-----------------------------------------------------------------|
| `make fmt`   | `cargo fmt --all -- --check`                                    |
| `make clippy`| `cargo clippy --all-targets -- -D warnings -A clippy::todo`     |
| `make test`  | `cargo test --verbose`                                          |
| `make wasm`  | `cargo build --target wasm32-unknown-unknown --release`         |
| `make build` | `cargo build --verbose` (quick debug build)                     |
| `make check` | all of the above, in order                                      |

### Auto-fix formatting

```bash
cargo fmt --all
```

Run this before committing if `make fmt` fails.

---

## Code style

- Follow `rustfmt.toml` settings; `make fmt` is the arbiter.
- No `clippy` warnings — `-D warnings` is enforced. The only exception is
  `clippy::todo`, which is allowed while scaffolded bodies are being filled in.
- Prefer `?` for error propagation over explicit `match` on `Result`.
- Keep `lib.rs` entry-points thin — delegate to the relevant module
  (`storage.rs`, `validation.rs`, `admin.rs`, `events.rs`).
- Write tests in `tests.rs` (or a dedicated `test_*.rs` file for a focused
  concern). One test per entry-point as a minimum; edge cases go in the same
  module.

---

## Doc-comment expectations

### Every `ContractError` variant must have a doc comment

Explain *when* the error fires and *who* receives it (caller or contract
internals). This makes the error index self-documenting and surfaces context
that would otherwise live only in PR threads.

```rust
/// Returned by `register_callback` when the idempotency key already exists
/// in temporary storage. The original `tx_id` is returned to the caller
/// instead of creating a duplicate record.
AlreadyRegistered = 2,
```

### Every event struct must have a doc comment

Explain *which entry-point* emits the event and the *minimum condition*
required for emission.

```rust
/// Emitted by `register_callback` when a new transaction is successfully
/// written to persistent storage. Not emitted for duplicate (idempotent)
/// calls that return an existing `tx_id`.
pub struct EventCallbackRegistered {
    pub tx_id: String,
    pub stellar_account: Address,
    pub ledger: u32,
}
```

### Public entry-points

Every `#[contractimpl]` method should have a doc comment covering:

1. **What it does** (one sentence).
2. **Access control** — who may call it.
3. **Errors** — list each `ContractError` variant it can return.
4. **Events** — list each event it may emit.

---

## Architecture Decision Records

Non-obvious decisions (trust model choices, storage tier trade-offs, auth
model splits, pause semantics, …) are recorded as Architecture Decision
Records in [`docs/adr/`](./docs/adr/).

Before opening a PR that makes a consequential design choice, check whether
an ADR already exists for it. If not, add one — the [`docs/adr/README.md`](./docs/adr/README.md)
has a template and guidance on what "consequential" means.

ADRs live forever and are never deleted. If a decision is reversed, add a new
ADR that supersedes the old one and link them.

---

## Issue tracker

- Bug reports and feature requests go in GitHub Issues.
- Assign yourself to an issue before starting work to avoid parallel efforts.
- Reference the issue in your PR description with `Closes #<n>`.
- If a PR only partially addresses an issue, use `Addresses #<n>` instead.
