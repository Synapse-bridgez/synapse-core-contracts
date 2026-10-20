//! Event definitions and emitters for the contract.
//!
//! Event topics are locked per `EVENTS.md`; the `init` topic is emitted exactly
//! once by `initialize(admin, relay_signer)`.

use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol};

/// Topic symbol for the contract-initialized event ("init" per README).
pub const TOPIC_INIT: Symbol = symbol_short!("init");

/// Payload emitted when the contract is initialized for the first time.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventContractInitialized {
    /// The admin address stored during initialization.
    pub admin: Address,
    /// The relay signer address stored during initialization.
    pub relay_signer: Address,
}

/// Emit the `EventContractInitialized` event under the "init" topic.
///
/// Called exactly once from `initialize`; the one-time-init guard in the
/// contract ensures this emitter is never reached on a second call.
pub fn emit_contract_initialized(env: &Env, admin: Address, relay_signer: Address) {
    let event = EventContractInitialized {
        admin,
        relay_signer,
    };
    env.events().publish((TOPIC_INIT,), event);
}
