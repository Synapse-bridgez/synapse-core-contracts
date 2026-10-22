//! Event emission helpers for the contract.
//!
//! # Guaranteed multi-event emission order
//!
//! Several entry points emit more than one event per invocation. For those
//! entry points the relative order of the emitted events is a documented,
//! tested guarantee (see `EVENTS.md`). The helpers in this module are the
//! single source of truth for those orderings: they publish events in a fixed
//! sequence so the guarantee is enforced at the source rather than relying on
//! call-site discipline.
//!
//! In particular, `complete_transaction` emits `status` **then** `done`; see
//! [`emit_status_then_done`].

use soroban_sdk::{Address, Env, Symbol};

/// Topic symbol for the `status` event.
pub const TOPIC_STATUS: &str = "status";
/// Topic symbol for the `done` event.
pub const TOPIC_DONE: &str = "done";

/// Emit the `status` event.
///
/// Publishes a `status` event with the given `tx_id` and `status` payload.
/// This is a low-level helper; callers that must also emit `done` should use
/// [`emit_status_then_done`] so the documented ordering is preserved.
pub fn emit_status(env: &Env, tx_id: u64, status: u32) {
    let topics = (Symbol::new(env, TOPIC_STATUS), tx_id);
    env.events().publish(topics, status);
}

/// Emit the `done` event.
///
/// Publishes a `done` event with the given `tx_id` and `stellar_tx_hash`
/// payload. This is a low-level helper; callers that must also emit `status`
/// should use [`emit_status_then_done`] so the documented ordering is
/// preserved.
pub fn emit_done(env: &Env, tx_id: u64, stellar_tx_hash: &soroban_sdk::BytesN<32>) {
    let topics = (Symbol::new(env, TOPIC_DONE), tx_id);
    env.events().publish(topics, stellar_tx_hash.clone());
}

/// Emit the `status` event followed by the `done` event, in that exact order.
///
/// This is the guaranteed emission order for `complete_transaction`
/// (`Processing → Completed`): the `status` event is published first, then the
/// `done` event. The ordering is enforced here so every caller inherits it.
///
/// # Arguments
///
/// * `env` - The contract environment.
/// * `tx_id` - The transaction identifier, used as the second topic of both
///   events.
/// * `status` - The new status payload for the `status` event.
/// * `stellar_tx_hash` - The Stellar transaction hash recorded on the `done`
///   event for audit purposes.
pub fn emit_status_then_done(
    env: &Env,
    tx_id: u64,
    status: u32,
    stellar_tx_hash: &soroban_sdk::BytesN<32>,
) {
    emit_status(env, tx_id, status);
    emit_done(env, tx_id, stellar_tx_hash);
}

/// Emit a generic single event with the given topic and payload.
///
/// Convenience wrapper used by entry points that emit a single event and
/// therefore make no ordering claim.
pub fn emit_single<T>(env: &Env, topic: &str, tx_id: u64, payload: T)
where
    T: soroban_sdk::IntoVal<Env, soroban_sdk::Val>,
{
    let topics = (Symbol::new(env, topic), tx_id);
    env.events().publish(topics, payload);
}

/// Emit an event with an `Address` payload (e.g. relay/admin notifications).
///
/// Convenience wrapper for single-event emitters whose payload is an address.
pub fn emit_address(env: &Env, topic: &str, tx_id: u64, payload: &Address) {
    let topics = (Symbol::new(env, topic), tx_id);
    env.events().publish(topics, payload.clone());
}