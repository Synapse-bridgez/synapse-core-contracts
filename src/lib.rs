//! # Recovery Contract
//!
//! This contract provides recovery and forwarding-route functionality for
//! transactions. Forwarding routes are stored for future Phase 2/3 polling
//! per the architecture diagram in the README.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec};

/// A forwarding route: an ordered list of destination addresses that a
/// future Phase 2/3 consumer may poll and act upon.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Route {
    pub hops: Vec<Address>,
}

/// Storage keys used by the contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Relay,
    ForwardingRoute(u64),
}

const FORWARDING_INTENT: Symbol = symbol_short!("fwd_intent");

#[contract]
pub struct RecoveryContract;

#[contractimpl]
impl RecoveryContract {
    /// Initialize the contract with an admin and relay signer.
    pub fn initialize(env: Env, admin: Address, relay: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Relay, &relay);
    }

    /// Set a forwarding route for `tx_id`.
    ///
    /// Only the relay signer or the admin may call this. The route is stored
    /// for future Phase 2/3 polling. A forwarding-intent event is emitted
    /// strictly last, after any other events this call produces, per the
    /// `route_emits_forwarding_intent_last` guarantee. Passing an empty route
    /// clears any stored route and emits no forwarding-intent event.
    pub fn set_forwarding_route(env: Env, tx_id: u64, route: Route, caller: Address) {
        caller.require_auth();

        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        let relay: Address = env.storage().instance().get(&DataKey::Relay).unwrap();
        if caller != admin && caller != relay {
            panic!("caller is not relay or admin");
        }

        if route.hops.is_empty() {
            // Clearing a route: no forwarding-intent event is emitted.
            env.storage()
                .persistent()
                .remove(&DataKey::ForwardingRoute(tx_id));
            return;
        }

        env.storage()
            .persistent()
            .set(&DataKey::ForwardingRoute(tx_id), &route);

        // Forwarding-intent event emitted strictly last.
        env.events()
            .publish((FORWARDING_INTENT, tx_id), route.hops.clone());
    }

    /// Read the forwarding route stored for `tx_id`, if any.
    ///
    /// This is an unauthenticated read.
    pub fn get_forwarding_route(env: Env, tx_id: u64) -> Option<Route> {
        env.storage()
            .persistent()
            .get(&DataKey::ForwardingRoute(tx_id))
    }
}
