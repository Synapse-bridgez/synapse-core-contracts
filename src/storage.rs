use soroban_sdk::{contracttype, Env, String, Symbol};

/// Storage keys for the parameter registry.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// A single parameter value keyed by its name.
    Param(String),
    /// The N-of-M relay-signer approval threshold.
    RelayThreshold,
}

/// Maximum length, in bytes, of a parameter name.
pub const MAX_PARAM_NAME_LEN: u32 = 32;

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
