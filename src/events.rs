//! Event emission helpers for the contract.
//!
//! Events are emitted via `env.events().publish(...)`. Ordering matters for
//! consumers that rely on a specific sequence (e.g. `complete_transaction`
//! guarantees "status then done", and `set_forwarding_route` guarantees the
//! forwarding-intent event fires strictly last).

use soroban_sdk::{contracttype, symbol_short, Address, BytesN, Env, Symbol};

/// Topic used for forwarding-intent events emitted by `set_forwarding_route`.
pub const FORWARDING_INTENT: Symbol = symbol_short!("fwd_intent");

/// Topic used for collateral-bonded events emitted by `bond_collateral`.
pub const COLLATERAL_BONDED: Symbol = symbol_short!("bonded");

/// Topic used for collateral-unbonded events emitted by `unbond_collateral`.
pub const COLLATERAL_UNBONDED: Symbol = symbol_short!("unbonded");

/// Topic used for unbond-claimed events emitted by `claim_unbond`.
pub const UNBOND_CLAIMED: Symbol = symbol_short!("claimed");

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

/// Emit a collateral-bonded event.
///
/// `amount` is the amount added by this call (the additive top-up), and
/// `total` is the signer's resulting bonded balance after the top-up.
pub fn emit_collateral_bonded(env: &Env, signer: Address, amount: i128, total: i128) {
    env.events()
        .publish((COLLATERAL_BONDED, signer), (amount, total));
}

/// Emit a collateral-unbonded event.
///
/// `amount` is the amount moved into the pending unbond request, and
/// `unlock_ledger` is the ledger at which `claim_unbond` becomes callable.
pub fn emit_collateral_unbonded(env: &Env, signer: Address, amount: i128, unlock_ledger: u32) {
    env.events()
        .publish((COLLATERAL_UNBONDED, signer), (amount, unlock_ledger));
}

/// Emit an unbond-claimed event.
///
/// `amount` is the amount released back to the signer by `claim_unbond`.
pub fn emit_unbond_claimed(env: &Env, signer: Address, amount: i128) {
    env.events().publish((UNBOND_CLAIMED, signer), amount);
}
