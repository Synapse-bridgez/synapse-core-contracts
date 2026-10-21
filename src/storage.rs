use soroban_sdk::{contracttype, Address, BytesN, Env, Vec};

use crate::types::{PendingUpgrade, UpgradeRecord};

/// Storage key for the in-flight timelocked upgrade proposal (issue #100).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpgradeKey {
    Pending,
    History,
}

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
