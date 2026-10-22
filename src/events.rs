//! Event emission helpers for the AnchorKit contract.
//!
//! Each helper emits a single contract event with a well-known topic so that
//! off-chain indexers can filter reliably. Topics are stable strings; payloads
//! are `(tx_id, ...)` tuples as documented per helper.

use soroban_sdk::{symbol_short, Env, String, Symbol};

/// Topic symbol used for transaction failure events.
///
/// Emitted by `fail_transaction` when an anchor genuinely fails. This is
/// deliberately distinct from the `cancel` topic: `fail` means "the anchor
/// genuinely failed", while `cancel` means "someone decided to stop this".
pub const FAIL_TOPIC: Symbol = symbol_short!("fail");

/// Emit the `fail` event for a transaction.
///
/// Publishes the `fail` topic with a payload of `(tx_id, reason)`, allowing
/// off-chain consumers to observe both the affected transaction and the
/// human-readable failure reason.
///
/// # Arguments
///
/// * `env` - The contract environment used to publish the event.
/// * `tx_id` - Identifier of the transaction that failed.
/// * `reason` - Human-readable reason for the failure. Callers are
///   responsible for validating `reason` against the string-length cap
///   (issue #47) before invoking this helper.
pub fn emit_fail(env: &Env, tx_id: u64, reason: &String) {
    env.events()
        .publish((FAIL_TOPIC, tx_id), reason.clone());
}