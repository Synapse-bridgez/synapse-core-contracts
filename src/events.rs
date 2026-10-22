//! Event definitions and emission helpers for the contract.
//!
//! Events are emitted with a stable topic so off-chain indexers can filter
//! reliably. The `init` topic is emitted exactly once by
//! `initialize(admin, relay_signer)`.
//!
//! The `upgrade` topic is emitted by the admin-gated `upgrade()`
//! entry point after a successful WASM swap (see `DECISIONS.md` §5).
//!
//! The `guardian_revoke` topic is emitted by the guardian emergency admin
//! revocation entry point described in
//! `docs/adr/0004-guardian-emergency-admin-revocation.md`. It is deliberately
//! distinct from the normal `propose_admin`/`accept_admin` transfer events so
//! downstream monitoring can tell a planned handoff apart from an emergency
//! revocation.
//!
//! Relay signer rotation events are emitted under the `relay` topic and mirror
//! the schema in `EVENTS.md`; keep them in sync.
//!
//! Events are emitted via `env.events().publish(...)`. Ordering matters for
//! consumers that rely on a specific sequence (e.g. `complete_transaction`
//! guarantees "status then done", and `set_forwarding_route` guarantees the
//! forwarding-intent event fires strictly last).

use soroban_sdk::{contracttype, symbol_short, Address, BytesN, Env, Symbol};

/// Topic symbol for the contract-initialized event ("init" per README).
pub const TOPIC_INIT: Symbol = symbol_short!("init");

/// Topic symbol used for contract-upgrade events.
pub const UPGRADE_TOPIC: Symbol = symbol_short!("upgrade");

/// Topic symbol used for guardian emergency admin revocation events.
pub const GUARDIAN_REVOKE_TOPIC: Symbol = symbol_short!("guardian_revoke");

/// Topic symbol used for relay-related events (matches the "relay" topic in
/// `EVENTS.md`).
pub const RELAY_TOPIC: Symbol = symbol_short!("relay");

/// Topic used for forwarding-intent events emitted by `set_forwarding_route`.
pub const FORWARDING_INTENT: Symbol = symbol_short!("fwd_intent");

/// Topic used for collateral-bonded events emitted by `bond_collateral`.
pub const COLLATERAL_BONDED: Symbol = symbol_short!("bonded");

/// Topic used for collateral-unbonded events emitted by `unbond_collateral`.
pub const COLLATERAL_UNBONDED: Symbol = symbol_short!("unbonded");

/// Topic used for unbond-claimed events emitted by `claim_unbond`.
pub const UNBOND_CLAIMED: Symbol = symbol_short!("claimed");

/// Payload emitted when the contract is initialized for the first time.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventContractInitialized {
    /// The admin address stored during initialization.
    pub admin: Address,
    /// The relay signer address stored during initialization.
    pub relay_signer: Address,
}

/// Payload emitted when the contract WASM is upgraded in place.
///
/// `new_wasm_hash` is the hash of the freshly installed WASM and
/// `schema_version` is the live schema version the upgrade was validated
/// against.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventContractUpgraded {
    pub new_wasm_hash: BytesN<32>,
    pub schema_version: u32,
}

/// Payload emitted when a guardian revokes a compromised admin.
///
/// `guardian` is the guardian that triggered the revocation, `revoked_admin`
/// is the admin that was removed, and `new_admin` is the replacement admin
/// that takes effect immediately.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventGuardianAdminRevoked {
    pub guardian: Address,
    pub revoked_admin: Address,
    pub new_admin: Address,
}

/// Payload emitted when the relay signer is rotated.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventRelaySignerRotated {
    /// The relay signer that was active before the rotation.
    pub old_signer: Address,
    /// The relay signer that is active after the rotation.
    pub new_signer: Address,
}

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

/// Emit the `upgrade` event for a successful in-place WASM swap.
///
/// Called by `upgrade()` only after the schema-version guard has passed and
/// `env.deployer().update_current_contract_wasm()` has been invoked.
pub fn emit_contract_upgraded(env: &Env, new_wasm_hash: BytesN<32>, schema_version: u32) {
    let payload = EventContractUpgraded {
        new_wasm_hash,
        schema_version,
    };
    env.events().publish((UPGRADE_TOPIC,), payload);
}

/// Emit the `guardian_revoke` event for a guardian emergency admin revocation.
///
/// Called by the guardian revocation entry point after the admin has been
/// replaced. The dedicated topic keeps this event distinguishable from the
/// normal `propose_admin`/`accept_admin` transfer events.
pub fn emit_guardian_admin_revoked(
    env: &Env,
    guardian: Address,
    revoked_admin: Address,
    new_admin: Address,
) {
    let payload = EventGuardianAdminRevoked {
        guardian,
        revoked_admin,
        new_admin,
    };
    env.events().publish((GUARDIAN_REVOKE_TOPIC,), payload);
}

/// Emit an [`EventRelaySignerRotated`] event under the `relay` topic.
///
/// This is emitted on every successful `set_relay_signer` call, including
/// no-op rotations to the currently-active signer.
pub fn emit_relay_signer_rotated(env: &Env, old_signer: Address, new_signer: Address) {
    let event = EventRelaySignerRotated {
        old_signer,
        new_signer,
    };
    env.events().publish((RELAY_TOPIC,), event);
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
