use soroban_sdk::{contracttype, Address, Env, String, Vec};

use crate::types::{DataKey, Transaction, TransactionStatus};

/// Maximum number of tags that may be attached to a single transaction.
///
/// Enforced by `add_transaction_tag` so that tag metadata stays bounded.
pub const MAX_TAGS_PER_TRANSACTION: u32 = 10;

/// Storage keys for the treasury two-party withdrawal flow (issue #284).
///
/// The two-party model is documented as accepted risk R-06 in
/// `THREAT_MODEL.md` §8: a withdrawal requires one authorized party to
/// propose and a *second, distinct* authorized party to authorize before
/// funds move.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreasuryKey {
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
        .get(&TreasuryKey::TreasuryBalance)
        .unwrap_or(0)
}

/// Persists the treasury balance.
pub fn set_treasury_balance(env: &Env, balance: i128) {
    env.storage()
        .instance()
        .set(&TreasuryKey::TreasuryBalance, &balance);
}

/// Returns the treasury configuration, if configured.
pub fn treasury_config(env: &Env) -> Option<TreasuryConfig> {
    env.storage().instance().get(&TreasuryKey::TreasuryConfig)
}

/// Persists the treasury configuration.
pub fn set_treasury_config(env: &Env, config: &TreasuryConfig) {
    env.storage().instance().set(&TreasuryKey::TreasuryConfig, config);
}

/// Returns the amount already withdrawn in the current epoch.
pub fn epoch_withdrawn(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&TreasuryKey::EpochWithdrawn)
        .unwrap_or(0)
}

/// Persists the amount withdrawn in the current epoch.
pub fn set_epoch_withdrawn(env: &Env, amount: i128) {
    env.storage()
        .instance()
        .set(&TreasuryKey::EpochWithdrawn, &amount);
}

/// Allocates and returns the next withdrawal id.
pub fn next_withdrawal_id(env: &Env) -> u64 {
    let id: u64 = env
        .storage()
        .instance()
        .get(&TreasuryKey::WithdrawalCounter)
        .unwrap_or(0);
    env.storage()
        .instance()
        .set(&TreasuryKey::WithdrawalCounter, &(id + 1));
    id
}

/// Stores a pending withdrawal under its id.
pub fn set_pending_withdrawal(env: &Env, id: u64, withdrawal: &PendingWithdrawal) {
    env.storage()
        .persistent()
        .set(&TreasuryKey::PendingWithdrawal(id), withdrawal);
}

/// Returns the pending withdrawal for `id`, if any.
pub fn pending_withdrawal(env: &Env, id: u64) -> Option<PendingWithdrawal> {
    env.storage()
        .persistent()
        .get(&TreasuryKey::PendingWithdrawal(id))
}

/// Removes a pending withdrawal once it has been authorized and settled.
pub fn remove_pending_withdrawal(env: &Env, id: u64) {
    env.storage()
        .persistent()
        .remove(&TreasuryKey::PendingWithdrawal(id));
}

/// Persist a transaction under its id.
pub fn set_transaction(env: &Env, tx_id: u64, tx: &Transaction) {
    env.storage()
        .persistent()
        .set(&DataKey::Transaction(tx_id), tx);
}

/// Load a transaction by id, if present.
pub fn get_transaction(env: &Env, tx_id: u64) -> Option<Transaction> {
    env.storage()
        .persistent()
        .get(&DataKey::Transaction(tx_id))
}

/// Return the tags currently attached to a transaction.
///
/// Returns an empty vector when the transaction has no tags yet.
pub fn get_transaction_tags(env: &Env, tx_id: u64) -> Vec<String> {
    env.storage()
        .persistent()
        .get(&DataKey::TransactionTags(tx_id))
        .unwrap_or_else(|| Vec::new(env))
}

/// Append a tag to a transaction's tag list.
///
/// Callers are expected to have validated the tag and the tag-count bound
/// before invoking this helper.
pub fn add_transaction_tag(env: &Env, tx_id: u64, tag: &String) {
    let mut tags = get_transaction_tags(env, tx_id);
    tags.push_back(tag.clone());
    env.storage()
        .persistent()
        .set(&DataKey::TransactionTags(tx_id), &tags);
}

/// Return the number of tags currently attached to a transaction.
pub fn transaction_tag_count(env: &Env, tx_id: u64) -> u32 {
    get_transaction_tags(env, tx_id).len()
}

/// Persist the status of a transaction.
pub fn set_transaction_status(env: &Env, tx_id: u64, status: &TransactionStatus) {
    if let Some(mut tx) = get_transaction(env, tx_id) {
        tx.status = status.clone();
        set_transaction(env, tx_id, &tx);
    }
}

/// Persist an arbitrary address-keyed flag.
pub fn set_flag(env: &Env, key: &DataKey, value: bool) {
    env.storage().persistent().set(key, &value);
}

/// Read an arbitrary address-keyed flag, defaulting to `false`.
pub fn get_flag(env: &Env, key: &DataKey) -> bool {
    env.storage().persistent().get(key).unwrap_or(false)
}

/// Persist the admin address.
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().persistent().set(&DataKey::Admin, admin);
}

/// Load the admin address, if configured.
pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().persistent().get(&DataKey::Admin)
}

/// Storage key for a signer's bond record.
#[soroban_sdk::contracttype]
#[derive(Clone)]
pub enum BondKey {
    Bond(Address),
    Unbond(Address),
}

/// Read the bond record for `signer`, returning a zero/default record when the
/// signer has never bonded.
pub fn get_bond_record(env: &Env, signer: &Address) -> BondRecord {
    env.storage()
        .persistent()
        .get(&BondKey::Bond(signer.clone()))
        .unwrap_or(BondRecord {
            signer: signer.clone(),
            amount: 0,
            bonded_at: 0,
        })
}

/// Read the pending unbond request for `signer`, or `None` when no request is
/// pending (including after a completed unbond has been claimed).
pub fn get_unbond_request(env: &Env, signer: &Address) -> Option<UnbondRequest> {
    env.storage()
        .persistent()
        .get(&BondKey::Unbond(signer.clone()))
}

pub fn get_relay_signer(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::RelaySigner)
}

/// Returns the effective relay signer set, applying the one-of-one fallback.
///
/// Decision (issue #304): a contract initialized before the N-of-M feature
/// existed must be representable as a valid one-of-one signer set without a
/// disruptive migration step. If the explicit N-of-M storage was never
/// initialized, this derives the equivalent one-of-one view from the legacy
/// single `relay_signer`.
pub fn relay_signer_set(env: &Env) -> Vec<Address> {
    if let Some(set) = get_relay_signer_set(env) {
        return set;
    }
    let mut out = Vec::new(env);
    if let Some(signer) = get_relay_signer(env) {
        out.push_back(signer);
    }
    out
}

/// Returns the current relay-signer set.
///
/// When the contract is still in the single-relay-signer baseline state (issue
/// #6) and no N-of-M set has been materialised yet, this returns a one-of-one
/// set containing the baseline signer, per ADR 0004's migration semantics.
pub fn get_relay_signers(env: &Env) -> Vec<Address> {
    if let Some(signers) = env.storage().instance().get(&DataKey::RelaySigners) {
        return signers;
    }

    // Baseline single-signer state: promote lazily into a one-of-one set.
    let mut signers = Vec::new(env);
    if let Some(signer) = env.storage().instance().get(&DataKey::RelaySigner) {
        signers.push_back(signer);
    }
    signers
}

/// Appends `new_signer` to the N-of-M relay-signer set, admin-gated.
///
/// If the contract is still in the single-relay-signer baseline state, the
/// first call transitions it into a one-of-one N-of-M set (containing the
/// baseline signer) before appending `new_signer`, yielding a one-of-two set.
///
/// Adding a signer that is already present is rejected with
/// [`Error::SignerAlreadyExists`] rather than silently no-op'ing, so callers
/// cannot mistake a duplicate for a successful rotation.
pub fn add_relay_signer(
    env: &Env,
    caller: &Address,
    new_signer: &Address,
) -> Result<(), Error> {
    caller.require_auth();

    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)?;
    if caller != &admin {
        return Err(Error::Unauthorized);
    }

    let mut signers = get_relay_signers(env);

    for existing in signers.iter() {
        if &existing == new_signer {
            return Err(Error::SignerAlreadyExists);
        }
    }

    signers.push_back(new_signer.clone());
    env.storage().instance().set(&DataKey::RelaySigners, &signers);

    events::relay_signer_added(env, new_signer);

    Ok(())
}

/// Returns the effective relay approval threshold, applying the one-of-one
/// fallback.
///
/// Decision (issue #304): when the explicit N-of-M threshold was never
/// initialized, a single-signer-only contract is treated as an implicit
/// one-of-one set with threshold `1`. Once the explicit N-of-M storage is
/// written, the stored threshold is authoritative.
pub fn relay_threshold(env: &Env) -> u32 {
    if let Some(threshold) = get_relay_threshold(env) {
        return threshold;
    }
    if get_relay_signer(env).is_some() {
        1
    } else {
        0
    }
}

/// Returns the quorum threshold for relay-call approvals.
///
/// Defaults to `1` when no threshold has been configured via
/// `set_relay_threshold`, matching the single-relay-signer baseline where one
/// approval is sufficient.
pub fn get_relay_threshold(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::RelayThreshold)
        .unwrap_or(1)
}

/// Returns the set of signers that have approved `call_id` so far.
///
/// Approvals live in `temporary` storage per the README's storage-tier table
/// ("Relay approvals"), so they are cheap to write and are consumed once the
/// quorum threshold is met.
pub fn get_relay_approvals(env: &Env, call_id: u64) -> Vec<Address> {
    env.storage()
        .temporary()
        .get(&DataKey::RelayApprovals(call_id))
        .unwrap_or_else(|| Vec::new(env))
}

/// Records `caller`'s approval of `call_id` and reports whether quorum is met.
///
/// `caller` must be a member of the current relay-signer set; otherwise
/// [`Error::Unauthorized`] is returned. A signer approving the same `call_id`
/// twice does not double-count toward quorum: the duplicate is a no-op and the
/// current approval count is re-evaluated against the threshold.
///
/// Returns `true` exactly when the number of distinct approvals reaches the
/// threshold set via `set_relay_threshold` (exactly-at-threshold succeeds,
/// threshold-minus-one does not). When quorum is met the approval set is
/// consumed from `temporary` storage so the call cannot be replayed.
pub fn approve_relay_call(
    env: &Env,
    call_id: u64,
    caller: &Address,
) -> Result<bool, Error> {
    caller.require_auth();

    let signers = get_relay_signers(env);
    let mut is_signer = false;
    for signer in signers.iter() {
        if &signer == caller {
            is_signer = true;
            break;
        }
    }
    if !is_signer {
        return Err(Error::Unauthorized);
    }

    let mut approvals = get_relay_approvals(env, call_id);

    let mut already_approved = false;
    for approved in approvals.iter() {
        if &approved == caller {
            already_approved = true;
            break;
        }
    }
    if !already_approved {
        approvals.push_back(caller.clone());
    }

    let threshold = get_relay_threshold(env);
    let quorum_met = approvals.len() >= threshold;

    if quorum_met {
        // Consume the approval set once quorum is reached.
        env.storage()
            .temporary()
            .remove(&DataKey::RelayApprovals(call_id));
    } else {
        env.storage()
            .temporary()
            .set(&DataKey::RelayApprovals(call_id), &approvals);
    }

    Ok(quorum_met)
}

/// Returns the slot-based status index for `status` (issue #24).
///
/// The index is a `Vec<u64>` of transaction ids in insertion order, stored
/// under [`DataKey::StatusIndex`]. Reading it is O(1) in the number of
/// transactions; only the requested page is materialised.
fn get_status_index(env: &Env, status: &TransactionStatus) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&DataKey::StatusIndex(status.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

/// Returns a page of transactions currently in `status`, unauthenticated.
///
/// Reads directly from the slot-based status index (issue #24) so the cost is
/// O(page-size) rather than O(total-transactions): only the `limit` ids in
/// `[offset, offset + limit)` are resolved to full [`Transaction`] records.
///
/// # Pagination and consistency
///
/// The index is append-ordered and each call re-reads the current index, so
/// the guaranteed consistency level is **eventually-consistent-per-ledger**:
/// within a single ledger the page is a consistent snapshot, but a transaction
/// that transitions out of `status` between two paginated calls may be absent
/// from (or shifted within) a later page. Callers that need a stable snapshot
/// across pages should pin to a ledger sequence and re-query on mismatch.
///
/// # Errors
///
/// Returns [`Error::InvalidLimit`] when `limit == 0` or
/// `limit > MAX_STATUS_PAGE_LIMIT`, per
/// `test_get_transactions_by_status_rejects_bad_limit`.
pub fn get_transactions_by_status(
    env: &Env,
    status: &TransactionStatus,
    offset: u32,
    limit: u32,
) -> Result<Vec<Transaction>, Error> {
    if limit == 0 || limit > MAX_STATUS_PAGE_LIMIT {
        return Err(Error::InvalidLimit);
    }

    let index = get_status_index(env, status);
    let total = index.len();

    let mut page = Vec::new(env);
    if offset >= total {
        return Ok(page);
    }

    let end = offset.saturating_add(limit).min(total);
    let mut i = offset;
    while i < end {
        let tx_id = index.get_unchecked(i);
        if let Some(tx) = env
            .storage()
            .persistent()
            .get::<DataKey, Transaction>(&DataKey::Transaction(tx_id))
        {
            // Skip entries that have transitioned out of `status` since the
            // index was last compacted; the index is eventually consistent.
            if tx.status == *status {
                page.push_back(tx);
            }
        }
        i += 1;
    }

    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{Address, Bytes, Env, String, Vec};

    #[test]
    fn round_trip_preserves_every_field() {
        let env = Env::default();
        let tx = Transaction {
            tx_id: Bytes::from_array(&env, &[1, 2, 3, 4]),
            status: TransactionStatus::Retrying,
            callback_type: CallbackType::OnFailure,
            amount: 1_000_000,
            stellar_account: Address::generate(&env),
            asset_code: String::from_str(&env, "USDC"),
            asset_issuer: Address::generate(&env),
            retry_count: 3,
            tags: Vec::new(&env),
        };

        let stored = pack(&tx);
        let restored = unpack(&stored);

        assert_eq!(restored, tx);
    }

    #[test]
    fn retry_count_defaults_to_zero_and_round_trips() {
        let env = Env::default();
        let tx = Transaction {
            tx_id: Bytes::from_array(&env, &[9, 9]),
            status: TransactionStatus::Pending,
            callback_type: CallbackType::None,
            amount: 0,
            stellar_account: Address::generate(&env),
            asset_code: String::from_str(&env, "XLM"),
            asset_issuer: Address::generate(&env),
            retry_count: 0,
            tags: Vec::new(&env),
        };

        let stored = pack(&tx);
        assert_eq!((stored.packed >> RETRY_SHIFT) & RETRY_MASK, 0);
        assert_eq!(unpack(&stored).retry_count, 0);
    }

    #[test]
    fn retry_count_is_masked_to_four_bits() {
        let env = Env::default();
        let tx = Transaction {
            tx_id: Bytes::from_array(&env, &[7]),
            status: TransactionStatus::Retrying,
            callback_type: CallbackType::OnComplete,
            amount: 5,
            stellar_account: Address::generate(&env),
            asset_code: String::from_str(&env, "USDC"),
            asset_issuer: Address::generate(&env),
            retry_count: 0xF,
            tags: Vec::new(&env),
        };

        let stored = pack(&tx);
        assert_eq!((stored.packed >> RETRY_SHIFT) & RETRY_MASK, 0xF);
        assert_eq!(unpack(&stored).retry_count, 0xF);
    }
}
}
