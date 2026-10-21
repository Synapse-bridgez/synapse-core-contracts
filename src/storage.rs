use soroban_sdk::{contracttype, Address, Env};

/// Storage keys for the treasury two-party withdrawal flow (issue #284).
///
/// The two-party model is documented as accepted risk R-06 in
/// `THREAT_MODEL.md` §8: a withdrawal requires one authorized party to
/// propose and a *second, distinct* authorized party to authorize before
/// funds move.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Monotonic counter used to allocate withdrawal ids.
    WithdrawalCounter,
    /// Pending withdrawal record keyed by withdrawal id.
    PendingWithdrawal(u64),
    /// Treasury configuration (authorized parties, epoch cap, etc.).
    TreasuryConfig,
    /// Current treasury balance.
    TreasuryBalance,
    /// Running total withdrawn in the current epoch.
    EpochWithdrawn,
}

/// A pending treasury withdrawal awaiting a second-party authorization.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingWithdrawal {
    /// Amount requested for withdrawal.
    pub amount: i128,
    /// Destination address that will receive the funds.
    pub destination: Address,
    /// The authorized party that proposed the withdrawal.
    pub proposer: Address,
}

/// Treasury configuration describing the authorized parties and epoch cap.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreasuryConfig {
    /// First authorized party (e.g. admin).
    pub party_a: Address,
    /// Second authorized party (e.g. treasury role).
    pub party_b: Address,
    /// Maximum amount that may be withdrawn per epoch.
    pub epoch_cap: i128,
}

/// Returns the current treasury balance.
pub fn treasury_balance(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::TreasuryBalance)
        .unwrap_or(0)
}

/// Persists the treasury balance.
pub fn set_treasury_balance(env: &Env, balance: i128) {
    env.storage()
        .instance()
        .set(&DataKey::TreasuryBalance, &balance);
}

/// Returns the treasury configuration, if configured.
pub fn treasury_config(env: &Env) -> Option<TreasuryConfig> {
    env.storage().instance().get(&DataKey::TreasuryConfig)
}

/// Persists the treasury configuration.
pub fn set_treasury_config(env: &Env, config: &TreasuryConfig) {
    env.storage().instance().set(&DataKey::TreasuryConfig, config);
}

/// Returns the amount already withdrawn in the current epoch.
pub fn epoch_withdrawn(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::EpochWithdrawn)
        .unwrap_or(0)
}

/// Persists the amount withdrawn in the current epoch.
pub fn set_epoch_withdrawn(env: &Env, amount: i128) {
    env.storage()
        .instance()
        .set(&DataKey::EpochWithdrawn, &amount);
}

/// Allocates and returns the next withdrawal id.
pub fn next_withdrawal_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .instance()
        .get(&DataKey::WithdrawalCounter)
        .unwrap_or(0);
    env.storage()
        .instance()
        .set(&DataKey::WithdrawalCounter, &(id + 1));
    id
}

/// Stores a pending withdrawal under its id.
pub fn set_pending_withdrawal(env: &Env, id: u64, withdrawal: &PendingWithdrawal) {
    env.storage()
        .persistent()
        .set(&DataKey::PendingWithdrawal(id), withdrawal);
}

/// Returns the pending withdrawal for `id`, if any.
pub fn pending_withdrawal(env: &Env, id: u64) -> Option<PendingWithdrawal> {
    env.storage()
        .persistent()
        .get(&DataKey::PendingWithdrawal(id))
}

/// Removes a pending withdrawal once it has been authorized and settled.
pub fn remove_pending_withdrawal(env: &Env, id: u64) {
    env.storage()
        .persistent()
        .remove(&DataKey::PendingWithdrawal(id));
}
