use soroban_sdk::{contracttype, Address, BytesN, Env, Vec};

use crate::types::{PendingUpgrade, UpgradeRecord};

/// Storage key for the in-flight timelocked upgrade proposal (issue #100).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpgradeKey {
    Pending,
    History,
    SchemaRange,
    UpgradeDelay,
}

/// Default minimum schema version when no compatibility range is configured.
///
/// A permissive default (0) is chosen so that an unconfigured contract does not
/// silently reject otherwise-valid upgrades; admins should set an explicit
/// range via `set_schema_compatibility_range` (issue #104) to tighten it.
pub const DEFAULT_SCHEMA_MIN: u32 = 0;

/// Default maximum schema version when no compatibility range is configured.
///
/// `u32::MAX` keeps the default range permissive (accept any schema) so that
/// upgrades are not blocked before an admin configures a range.
pub const DEFAULT_SCHEMA_MAX: u32 = u32::MAX;

/// Default upgrade timelock delay (in ledgers) when none is configured.
///
/// Zero means no enforced delay until an admin sets one via
/// `set_upgrade_delay` (issue #105).
pub const DEFAULT_UPGRADE_DELAY: u32 = 0;

/// Persist the in-flight timelocked upgrade proposal.
pub fn set_pending_upgrade(env: &Env, pending: &PendingUpgrade) {
    env.storage()
        .persistent()
        .set(&UpgradeKey::Pending, pending);
}

/// Read the in-flight timelocked upgrade proposal, if any.
pub fn get_pending_upgrade(env: &Env) -> Option<PendingUpgrade> {
    env.storage().persistent().get(&UpgradeKey::Pending)
}

/// Clear the in-flight timelocked upgrade proposal (after finalize/cancel).
pub fn clear_pending_upgrade(env: &Env) {
    env.storage().persistent().remove(&UpgradeKey::Pending);
}

/// Append a completed upgrade to the history backing store (issue #27).
pub fn push_upgrade_history(env: &Env, record: &UpgradeRecord) {
    let mut history: Vec<UpgradeRecord> = env
        .storage()
        .persistent()
        .get(&UpgradeKey::History)
        .unwrap_or_else(|| Vec::new(env));
    history.push_back(record.clone());
    env.storage()
        .persistent()
        .set(&UpgradeKey::History, &history);
}

/// Read the full upgrade history backing store (issue #27).
pub fn get_upgrade_history(env: &Env) -> Vec<UpgradeRecord> {
    env.storage()
        .persistent()
        .get(&UpgradeKey::History)
        .unwrap_or_else(|| Vec::new(env))
}

/// Read the most recently completed upgrade, if any.
pub fn previous_upgrade(env: &Env) -> Option<UpgradeRecord> {
    let history = get_upgrade_history(env);
    history.last()
}

/// Persist the configured schema compatibility range (min, max) (issue #104).
pub fn set_schema_compatibility_range(env: &Env, min: u32, max: u32) {
    env.storage()
        .persistent()
        .set(&UpgradeKey::SchemaRange, &(min, max));
}

/// Read the configured schema compatibility range (min, max) (issue #55/#104).
///
/// Returns `(DEFAULT_SCHEMA_MIN, DEFAULT_SCHEMA_MAX)` when unset, i.e. a
/// permissive range that accepts any schema version.
pub fn get_schema_compatibility_range(env: &Env) -> (u32, u32) {
    env.storage()
        .persistent()
        .get(&UpgradeKey::SchemaRange)
        .unwrap_or((DEFAULT_SCHEMA_MIN, DEFAULT_SCHEMA_MAX))
}

/// Persist the configured upgrade timelock delay in ledgers (issue #105).
pub fn set_upgrade_delay(env: &Env, delay: u32) {
    env.storage()
        .persistent()
        .set(&UpgradeKey::UpgradeDelay, &delay);
}

/// Read the configured upgrade timelock delay in ledgers (issue #105).
///
/// Returns `DEFAULT_UPGRADE_DELAY` (0) when unset, i.e. no enforced delay.
pub fn get_upgrade_delay(env: &Env) -> u32 {
    env.storage()
        .persistent()
        .get(&UpgradeKey::UpgradeDelay)
        .unwrap_or(DEFAULT_UPGRADE_DELAY)
}
