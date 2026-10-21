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

/// A pending collateral-unbond request, finalized by `claim_unbond` once the
/// configured `unbond_delay_ledgers` has elapsed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnbondRequest {
    pub amount: i128,
    pub unlock_ledger: u32,
}

/// Storage keys used by the contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Relay,
    ForwardingRoute(u64),
    Bond(Address),
    Unbond(Address),
    UnbondDelayLedgers,
}

const FORWARDING_INTENT: Symbol = symbol_short!("fwd_intent");
const BONDED: Symbol = symbol_short!("bonded");
const UNBOND_INIT: Symbol = symbol_short!("unbond_init");
const UNBOND_CLAIM: Symbol = symbol_short!("unbond_claim");

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

    /// Bond `amount` of collateral for `signer`.
    ///
    /// Self-authorized: `signer` must authorize this call for themselves,
    /// distinct from the relay-or-admin pattern used elsewhere. Bonding is
    /// additive (a top-up) per issue #29: the new amount is added to any
    /// existing bond record for `signer`.
    pub fn bond_collateral(env: Env, signer: Address, amount: i128) {
        signer.require_auth();
        if amount <= 0 {
            panic!("amount must be positive");
        }

        let current: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Bond(signer.clone()))
            .unwrap_or(0);
        let updated = current + amount;
        env.storage()
            .persistent()
            .set(&DataKey::Bond(signer.clone()), &updated);

        env.events()
            .publish((BONDED, signer), (amount, updated));
    }

    /// Initiate a timelocked unbond of `amount` collateral for `signer`.
    ///
    /// Self-authorized. The bonded balance is reduced immediately and a
    /// pending `UnbondRequest` is stored, unlockable after
    /// `unbond_delay_ledgers` (from the param registry) has elapsed. Unbonding
    /// more than the currently-bonded amount is rejected.
    pub fn unbond_collateral(env: Env, signer: Address, amount: i128) {
        signer.require_auth();
        if amount <= 0 {
            panic!("amount must be positive");
        }

        let current: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Bond(signer.clone()))
            .unwrap_or(0);
        if amount > current {
            panic!("unbond exceeds bonded amount");
        }

        let delay: u32 = env
            .storage()
            .instance()
            .get(&DataKey::UnbondDelayLedgers)
            .unwrap_or(0);
        let unlock_ledger = env.ledger().sequence() + delay;

        env.storage()
            .persistent()
            .set(&DataKey::Bond(signer.clone()), &(current - amount));
        env.storage().persistent().set(
            &DataKey::Unbond(signer.clone()),
            &UnbondRequest {
                amount,
                unlock_ledger,
            },
        );

        env.events()
            .publish((UNBOND_INIT, signer), (amount, unlock_ledger));
    }

    /// Finalize a pending unbond request for `signer`.
    ///
    /// Self-authorized. Rejected before the delay elapses; succeeds exactly at
    /// the delay boundary (`ledger >= unlock_ledger`).
    pub fn claim_unbond(env: Env, signer: Address) {
        signer.require_auth();

        let request: UnbondRequest = env
            .storage()
            .persistent()
            .get(&DataKey::Unbond(signer.clone()))
            .unwrap_or_else(|| panic!("no pending unbond"));

        if env.ledger().sequence() < request.unlock_ledger {
            panic!("unbond delay has not elapsed");
        }

        env.storage()
            .persistent()
            .remove(&DataKey::Unbond(signer.clone()));

        env.events()
            .publish((UNBOND_CLAIM, signer), request.amount);
    }
}
