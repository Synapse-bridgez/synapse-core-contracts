use soroban_sdk::{contracttype, Address, Env, String, Symbol};

/// Storage keys for the parameter registry.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// A single parameter value keyed by its name.
    Param(String),
    /// The N-of-M relay-signer approval threshold.
    RelayThreshold,
    /// The ledger-count delay enforced by the timelocked relay-signer rotation.
    RelaySignerDelay,
    /// The pending (proposed) relay signer awaiting timelock finalization.
    PendingRelaySigner,
    /// The ledger sequence at which the pending relay-signer change finalizes.
    RelaySignerFinalizeAt,
}

/// Maximum length, in bytes, of a parameter name.
pub const MAX_PARAM_NAME_LEN: u32 = 32;

/// Minimum allowed value, in ledgers, for the relay-signer rotation delay.
///
/// A zero (or near-zero) delay would defeat the timelock's purpose entirely,
/// so the smallest accepted delay is one ledger.
pub const MIN_RELAY_SIGNER_DELAY: u32 = 1;

/// Maximum allowed value, in ledgers, for the relay-signer rotation delay.
///
/// Bounds the delay to roughly one year of ledgers (~5s each) so a misconfigured
/// value cannot lock the rotation flow out indefinitely.
pub const MAX_RELAY_SIGNER_DELAY: u32 = 6_307_200;

/// Reads a parameter from persistent storage.
///
/// Returns `Some(value)` when the parameter has been set, and `None` when the
/// key has never been written. Callers must treat `None` as an explicit
/// "not found" signal rather than substituting a default value.
pub fn get_param(env: &Env, name: &String) -> Option<i128> {
    let key = DataKey::Param(name.clone());
    env.storage().persistent().get(&key)
}

/// Writes a parameter to persistent storage.
pub fn set_param(env: &Env, name: &String, value: i128) {
    let key = DataKey::Param(name.clone());
    env.storage().persistent().set(&key, &value);
}

/// Returns `true` when the parameter has been set.
pub fn has_param(env: &Env, name: &String) -> bool {
    let key = DataKey::Param(name.clone());
    env.storage().persistent().has(&key)
}

/// Removes a parameter from persistent storage, if present.
pub fn remove_param(env: &Env, name: &String) {
    let key = DataKey::Param(name.clone());
    env.storage().persistent().remove(&key);
}

/// Returns the well-known parameter names used by the registry.
pub fn well_known_keys(env: &Env) -> [String; 3] {
    [
        String::from_str(env, "fee_treasury"),
        String::from_str(env, "relay_signer"),
        String::from_str(env, "timelock"),
    ]
}

/// Returns `true` when `name` is within the allowed byte length.
pub fn is_valid_param_name(name: &String) -> bool {
    name.len() <= MAX_PARAM_NAME_LEN
}

/// Emits a symbol used for parameter-related events.
pub fn param_event_symbol(env: &Env) -> Symbol {
    Symbol::new(env, "param")
}

/// Reads the configured N-of-M relay-signer approval threshold.
///
/// Returns `None` when no threshold has been configured yet.
pub fn get_relay_threshold(env: &Env) -> Option<u32> {
    env.storage().persistent().get(&DataKey::RelayThreshold)
}

/// Persists the N-of-M relay-signer approval threshold.
///
/// Callers are responsible for validating `threshold` against the current
/// signer set (see issue #93's bounds check) before writing.
pub fn set_relay_threshold(env: &Env, threshold: u32) {
    env.storage()
        .persistent()
        .set(&DataKey::RelayThreshold, &threshold);
}

/// Returns `true` when a relay-signer threshold has been configured.
pub fn has_relay_threshold(env: &Env) -> bool {
    env.storage().persistent().has(&DataKey::RelayThreshold)
}

/// Removes the configured relay-signer threshold, if present.
pub fn remove_relay_threshold(env: &Env) {
    env.storage().persistent().remove(&DataKey::RelayThreshold);
}

/// Reads the configured relay-signer rotation delay, in ledgers.
///
/// Returns `None` when no delay has been configured yet.
pub fn get_relay_signer_delay(env: &Env) -> Option<u32> {
    env.storage().persistent().get(&DataKey::RelaySignerDelay)
}

/// Persists the relay-signer rotation delay, in ledgers.
///
/// Callers are responsible for validating `delay_ledgers` against
/// [`MIN_RELAY_SIGNER_DELAY`] and [`MAX_RELAY_SIGNER_DELAY`] before writing.
pub fn set_relay_signer_delay(env: &Env, delay_ledgers: u32) {
    env.storage()
        .persistent()
        .set(&DataKey::RelaySignerDelay, &delay_ledgers);
}

/// Returns `true` when a relay-signer rotation delay has been configured.
pub fn has_relay_signer_delay(env: &Env) -> bool {
    env.storage().persistent().has(&DataKey::RelaySignerDelay)
}

/// Removes the configured relay-signer rotation delay, if present.
pub fn remove_relay_signer_delay(env: &Env) {
    env.storage().persistent().remove(&DataKey::RelaySignerDelay);
}

/// Reads the pending relay signer awaiting timelock finalization.
///
/// Returns `None` when no rotation is currently proposed.
pub fn get_pending_relay_signer(env: &Env) -> Option<Address> {
    env.storage().persistent().get(&DataKey::PendingRelaySigner)
}

/// Persists the pending relay signer and the ledger sequence at which the
/// timelocked rotation becomes finalizable.
///
/// Callers are responsible for computing `finalize_at` from the configured
/// `set_relay_signer_delay` (issue #88) before writing.
pub fn set_pending_relay_signer(env: &Env, signer: &Address, finalize_at: u32) {
    env.storage()
        .persistent()
        .set(&DataKey::PendingRelaySigner, signer);
    env.storage()
        .persistent()
        .set(&DataKey::RelaySignerFinalizeAt, &finalize_at);
}

/// Returns the ledger sequence at which the pending relay-signer change
/// finalizes, or `None` when no rotation is pending.
pub fn get_relay_signer_finalize_at(env: &Env) -> Option<u32> {
    env.storage()
        .persistent()
        .get(&DataKey::RelaySignerFinalizeAt)
}

/// Returns `true` when a relay-signer rotation is currently pending.
pub fn has_pending_relay_signer(env: &Env) -> bool {
    env.storage()
        .persistent()
        .has(&DataKey::PendingRelaySigner)
}

/// Clears any pending relay-signer rotation (used on finalize and cancel).
pub fn remove_pending_relay_signer(env: &Env) {
    env.storage()
        .persistent()
        .remove(&DataKey::PendingRelaySigner);
    env.storage()
        .persistent()
        .remove(&DataKey::RelaySignerFinalizeAt);
}
