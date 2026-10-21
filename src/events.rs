//! Event emission helpers for the contract.
//!
//! Events are emitted via `env.events().publish(...)`. Ordering matters for
//! consumers that rely on a specific sequence (e.g. `complete_transaction`
//! guarantees "status then done", and `set_forwarding_route` guarantees the
//! forwarding-intent event fires strictly last).

use soroban_sdk::{contracttype, symbol_short, Address, BytesN, Env, Symbol};

/// Topic used for forwarding-intent events emitted by `set_forwarding_route`.
pub const FORWARDING_INTENT: Symbol = symbol_short!("fwd_intent");

/// Payload for a forwarding-intent event.
///
/// Emitted strictly last by `set_forwarding_route` so downstream consumers
/// (future Phase 2/3 pollers) can rely on it being the final event of the call.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForwardingIntent {
    /// The transaction id the forwarding route was set for.
    pub tx_id: BytesN<32>,
    /// The caller that set the route (relay or admin).
    pub caller: Address,
}

/// Emit a forwarding-intent event.
///
/// Callers MUST invoke this only after every other event the surrounding
/// operation produces, to preserve the `route_emits_forwarding_intent_last`
/// ordering guarantee. When no route is set (or a route is cleared), this
/// helper must not be called at all.
pub fn emit_forwarding_intent(env: &Env, tx_id: BytesN<32>, caller: Address) {
    let intent = ForwardingIntent { tx_id, caller };
    env.events().publish((FORWARDING_INTENT,), intent);
}
