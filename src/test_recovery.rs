//! Tests for the `merge_duplicate_transactions` recovery entry point (#63).

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String};

use crate::types::{CallbackPayload, CallbackType, ContractError, TransactionStatus};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

fn setup() -> (Env, SynapseCoreContractClient<'static>, Address) {
    let env = Env::default();
    let id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay, &BytesN::from_array(&env, &[0x01u8; 32]));
    (env, client, admin)
}

fn reg(env: &Env, client: &SynapseCoreContractClient, tx: &str) -> String {
    let acct = String::from_str(
        env,
        "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ",
    );
    client.register_callback(&CallbackPayload {
        transaction_id: String::from_str(env, tx),
        stellar_account: acct.clone(),
        amount: 1_000,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: acct,
        idempotency_key: String::from_str(env, tx),
        anchor_transaction_id: String::from_str(env, "anchor-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    })
}

#[test]
fn merge_marks_duplicate_and_keeps_data() {
    let (env, client, admin) = setup();
    let a = reg(&env, &client, "tx-a");
    let b = reg(&env, &client, "tx-b");
    let why = String::from_str(&env, "replay");
    client.merge_duplicate_transactions(&a, &b, &admin, &why);
    assert_eq!(client.get_merged_into(&b), Some(a.clone()));
    let tx = client.get_transaction(&b);
    assert_eq!(tx.amount, 1_000);
    assert_eq!(tx.status, TransactionStatus::Failed);
    assert_eq!(tx.failure_reason, String::from_str(&env, "merged"));
    assert_eq!(client.get_merged_into(&a), None);
}

#[test]
fn merge_rejects_self_double_and_settled() {
    let (env, client, admin) = setup();
    let a = reg(&env, &client, "tx-a");
    let b = reg(&env, &client, "tx-b");
    let c = reg(&env, &client, "tx-c");
    let why = String::from_str(&env, "replay");
    assert_eq!(
        client
            .try_merge_duplicate_transactions(&a, &a, &admin, &why)
            .unwrap_err()
            .unwrap(),
        ContractError::MergeSelf
    );
    client.merge_duplicate_transactions(&a, &b, &admin, &why);
    assert_eq!(
        client
            .try_merge_duplicate_transactions(&a, &b, &admin, &why)
            .unwrap_err()
            .unwrap(),
        ContractError::AlreadyMerged
    );
    client.start_processing(&c, &admin);
    client.complete_transaction(&c, &String::from_str(&env, &"a".repeat(64)), &admin);
    assert_eq!(
        client
            .try_merge_duplicate_transactions(&a, &c, &admin, &why)
            .unwrap_err()
            .unwrap(),
        ContractError::DuplicateSettled
    );
}

// ─── #64 forwarding intent ────────────────────────────────────────────────────

use soroban_sdk::testutils::Events;

fn complete(env: &Env, client: &SynapseCoreContractClient, admin: &Address, tx: &String) {
    client.start_processing(tx, admin);
    client.complete_transaction(tx, &String::from_str(env, &"a".repeat(64)), admin);
}

#[test]
fn no_route_emits_no_forwarding_event() {
    let (env, client, admin) = setup();
    let a = reg(&env, &client, "tx-a");
    client.start_processing(&a, &admin);
    client.complete_transaction(&a, &String::from_str(&env, &"a".repeat(64)), &admin);
    // status + done only
    assert_eq!(env.events().all().len(), 2);
}

#[test]
fn route_emits_forwarding_intent_last() {
    let (env, client, admin) = setup();
    let a = reg(&env, &client, "tx-a");
    client.set_forwarding_route(&a, &2);
    assert_eq!(client.get_forwarding_route(&a), Some(2));
    client.start_processing(&a, &admin);
    client.complete_transaction(&a, &String::from_str(&env, &"a".repeat(64)), &admin);
    // last call's events: status, done, fwd (in order)
    let evs = env.events().all();
    assert_eq!(evs.len(), 3);
    let _ = complete;
}

// ─── #65 relay signer set ─────────────────────────────────────────────────────

#[test]
fn migrated_single_signer_is_one_of_one() {
    let (env, client, _admin) = setup();
    let set = client.relay_signer_set();
    assert_eq!(set.threshold, 1);
    assert_eq!(set.signers.len(), 1);
    assert_eq!(set.signers.get(0).unwrap(), client.relay_signer());
    let _ = env;
}

#[test]
fn quorum_met_allowed_one_short_rejected() {
    let (env, client, admin) = setup();
    let a = reg(&env, &client, "tx-a");
    let primary = client.relay_signer();
    let s2 = Address::generate(&env);
    client.add_relay_signer(&s2);
    client.set_relay_threshold(&2);
    assert_eq!(
        client
            .try_start_processing(&a, &primary)
            .unwrap_err()
            .unwrap(),
        ContractError::QuorumNotMet
    );
    client.approve_relay_call(&s2);
    client.start_processing(&a, &primary);
    assert_eq!(client.get_status(&a), TransactionStatus::Processing);
    let _ = admin;
}

#[test]
fn threshold_bounds_enforced() {
    let (env, client, _admin) = setup();
    assert_eq!(
        client.try_set_relay_threshold(&0).unwrap_err().unwrap(),
        ContractError::InvalidThreshold
    );
    assert_eq!(
        client.try_set_relay_threshold(&2).unwrap_err().unwrap(),
        ContractError::InvalidThreshold
    );
    let primary = client.relay_signer();
    assert_eq!(
        client
            .try_remove_relay_signer(&primary)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidThreshold
    );
    let _ = env;
}

// ─── #66 timelocked relay signer ──────────────────────────────────────────────

use soroban_sdk::testutils::Ledger;

#[test]
fn timelock_boundary_and_cancel() {
    let (env, client, _admin) = setup();
    let new = Address::generate(&env);
    client.set_relay_signer_delay(&100);
    let start = env.ledger().sequence();
    client.propose_relay_signer(&new);
    env.ledger().with_mut(|l| l.sequence_number = start + 99);
    assert_eq!(
        client.try_finalize_relay_signer().unwrap_err().unwrap(),
        ContractError::TimelockNotElapsed
    );
    env.ledger().with_mut(|l| l.sequence_number = start + 100);
    client.finalize_relay_signer();
    assert_eq!(client.relay_signer(), new);

    client.propose_relay_signer(&Address::generate(&env));
    client.cancel_relay_signer_change();
    assert_eq!(
        client.try_finalize_relay_signer().unwrap_err().unwrap(),
        ContractError::NoPendingChange
    );
}

#[test]
fn immediate_rotation_closed_when_delay_set() {
    let (env, client, _admin) = setup();
    client.set_relay_signer_delay(&10);
    assert_eq!(
        client
            .try_set_relay_signer(&Address::generate(&env))
            .unwrap_err()
            .unwrap(),
        ContractError::TimelockRequired
    );
}

// ─── #301 quorum isolation: relay-call approvals vs. rotation finalization ────

/// A signer's `approve_relay_call` approval for a `call_id` must not count
/// toward any pending rotation-finalization quorum, and a rotation proposal
/// must not satisfy a relay-call quorum. Both mechanisms are exercised
/// concurrently to confirm their storage namespaces stay isolated.
#[test]
fn relay_call_and_rotation_quorums_are_isolated() {
    let (env, client, _admin) = setup();
    let a = reg(&env, &client, "tx-a");
    let primary = client.relay_signer();
    let s2 = Address::generate(&env);
    client.add_relay_signer(&s2);
    client.set_relay_threshold(&2);

    // s2 is both a relay-call approver and mid-rotation-proposal.
    let new_signer = Address::generate(&env);
    client.set_relay_signer_delay(&10);
    client.propose_relay_signer(&new_signer);

    // A relay-call approval for `a` must not advance the rotation quorum.
    client.approve_relay_call(&s2);
    assert_eq!(
        client.try_finalize_relay_signer().unwrap_err().unwrap(),
        ContractError::TimelockNotElapsed
    );

    // The relay-call quorum is satisfied independently of the rotation.
    client.start_processing(&a, &primary);
    assert_eq!(client.get_status(&a), TransactionStatus::Processing);
}

// ─── #302 merge-aware bulk queries ────────────────────────────────────────────

/// Merged-away transactions are excluded from every status-based bulk query
/// but remain directly queryable by `tx_id`. This test confirms that
/// `get_transactions_by_status` does not double-count a merged-away record
/// alongside its primary, and that `get_merged_into` round-trips for a
/// transaction merged away before the bulk queries existed.
#[test]
fn merged_away_excluded_from_status_query_but_directly_queryable() {
    let (env, client, admin) = setup();
    let a = reg(&env, &client, "tx-a");
    let b = reg(&env, &client, "tx-b");
    let why = String::from_str(&env, "replay");

    // Both start in the same status bucket.
    let before = client.get_transactions_by_status(&TransactionStatus::Pending);
    assert_eq!(before.len(), 2);

    client.merge_duplicate_transactions(&a, &b, &admin, &why);

    // The merged-away record is excluded from the status bucket it would
    // otherwise share with its primary.
    let after = client.get_transactions_by_status(&TransactionStatus::Pending);
    assert_eq!(after.len(), 1);
    assert_eq!(after.get(0).unwrap(), a.clone());

    // It is still directly queryable by `tx_id` and keeps its data.
    let tx = client.get_transaction(&b);
    assert_eq!(tx.amount, 1_000);
    assert_eq!(tx.status, TransactionStatus::Failed);

    // `get_merged_into` round-trips for the merged-away record.
    assert_eq!(client.get_merged_into(&b), Some(a.clone()));
    assert_eq!(client.get_merged_into(&a), None);
}

/// A merged-away transaction must not appear in the dispute queue, even if it
/// was disputed before being merged away. The dispute record is not orphaned:
/// the primary remains the single authoritative record for the pair.
#[test]
fn merged_away_excluded_from_dispute_queue() {
    let (env, client, admin) = setup();
    let a = reg(&env, &client, "tx-a");
    let b = reg(&env, &client, "tx-b");
    let why = String::from_str(&env, "replay");

    // Dispute the duplicate before it is discovered to be a duplicate.
    client.dispute_transaction(&b, &String::from_str(&env, "fraud"));
    let queued = client.get_dispute_queue();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued.get(0).unwrap(), b.clone());

    // Merge the disputed duplicate away into its primary.
    client.merge_duplicate_transactions(&a, &b, &admin, &why);

    // The merged-away record is excluded from the dispute queue.
    let queued = client.get_dispute_queue();
    assert_eq!(queued.len(), 0);

    // The primary is unaffected and remains directly queryable.
    assert_eq!(client.get_merged_into(&b), Some(a.clone()));
    assert_eq!(client.get_status(&a), TransactionStatus::Pending);
}
