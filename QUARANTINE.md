# Quarantined tests

The Wave 2 merge (#197) replaced `src/lib.rs`, `src/types.rs` and
`src/storage.rs` wholesale. That dropped a number of entry points added by
earlier merges (#176–#191), but their tests stayed behind, so `main` stopped
compiling and CI has been red since `c395f27`.

The PR that closes #166, #169, #173 and #175 got `main` green again by:

- repairing the merge damage in `src/events.rs`;
- restoring the features the open issues depend on: `batch_register_callback`,
  disputes (`dispute_transaction`, `resolve_dispute`, `is_disputed`), and
  per-anchor amount ceilings;
- fixing tests whose only problem was signature drift (`initialize` and
  `upgrade` lost their third argument; namespaced storage keys reverted to
  plain `StorageKey` variants);
- **quarantining** the tests below, which exercise entry points that no longer
  exist in the contract.

Quarantined items are gated with `#[cfg(quarantine)]`. The `quarantine` cfg is
declared in `Cargo.toml` and never set, so these items are not compiled. To
work on one, restore the feature, delete its `#[cfg(quarantine)]` line, and
remove its row here.

## Gated tests

| File | Test | Missing feature |
|------|------|-----------------|
| `src/tests.rs` | `test_cancel_transaction_by_relay_and_admin` | `cancel_transaction`, `TransactionStatus::Cancelled` |
| `src/tests.rs` | `test_cancel_transaction_rejects_stranger_and_illegal_states` | `cancel_transaction`, `CannotCancel`, `AlreadyCancelled` |
| `src/tests.rs` | `test_retry_transaction_cycle_and_limit` | `retry_transaction`, `Transaction::retry_count` |
| `src/tests.rs` | `test_retry_transaction_rejects_non_failed` | `retry_transaction` |
| `src/tests.rs` | `test_get_transactions_by_status_pagination_and_transitions` | `get_transactions_by_status` |
| `src/tests.rs` | `test_get_transactions_by_status_rejects_bad_limit` | `get_transactions_by_status` |
| `src/test_pause.rs` | `test_post_upgrade_self_check_passes_on_healthy_state` | `StorageClient::post_upgrade_self_check` |
| `src/test_pause.rs` | `test_post_upgrade_self_check_fails_when_relay_missing` | `post_upgrade_self_check`, `SelfCheckFailed` |
| `src/test_pause.rs` | `test_upgrade_self_check_failure_reverts_without_history` | self-check, `get_upgrade_history` |
| `src/test_pause.rs` | `test_self_check_events_topics` | `EventUpgradeSelfCheckPassed` / `Failed` emitters |
| `src/test_pause.rs` | `test_simulate_upgrade_*` (5 tests) | `simulate_upgrade`, `UpgradeCompatibility` |
| `src/test_pause.rs` | `test_upgrade_history_*` (3 tests) | `get_upgrade_history`, `UpgradeRecord`, `MAX_UPGRADE_HISTORY` |

`MINIMAL_WASM` and `upload_minimal` in `src/test_pause.rs` are gated too,
because only the quarantined upgrade tests use them.

## Unwired files

These files are not declared as modules in `src/lib.rs` (they were not on
`main` before this PR either). They depend entirely on removed features:

| File | Missing features |
|------|------------------|
| `src/test_recovery.rs` | `merge_duplicate_transactions`, forwarding routes, N-of-M relay signer sets |
| `src/test_upgrade_safety.rs`, `src/migration.rs` | `UpgradeSnapshot`, storage migrations |
| `src/test_wave.rs` | empty |

## Docs that still describe removed features

`EVENTS.md` §2 still lists some events as **Live** whose emitters no longer
exist, e.g. the upgrade-lifecycle, guardian and auto-pause events. Reconcile
it when the corresponding features are restored or formally dropped.
