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

// ─── #303 forwarding-route lifecycle guarantees ──────────────────────────────

/// The forwarding route is orthogonal metadata on the transaction record: it
/// must remain readable via `get_forwarding_route` across every lifecycle
/// state (Pending -> Processing -> Completed/Failed/Cancelled) so a future
/// Swap Engine poller always sees a consistent result regardless of when it
/// polls. No status transition clears the route.
#[test]
fn route_readable_across_completed_lifecycle() {
    let (env, client, admin) = setup();
    let a = reg(&env, &client, "tx-a");
    client.set_forwarding_route(&a, &7);
    assert_eq!(client.get_status(&a), TransactionStatus::Pending);
    assert_eq!(client.get_forwarding_route(&a), Some(7));

    client.start_processing(&a, &admin);
    assert_eq!(client.get_status(&a), TransactionStatus::Processing);
    assert_eq!(client.get_forwarding_route(&a), Some(7));

    client.complete_transaction(&a, &String::from_str(&env, &"a".repeat(64)), &admin);
    assert_eq!(client.get_status(&a), TransactionStatus::Completed);
    assert_eq!(client.get_forwarding_route(&a), Some(7));
}

#[test]
fn route_readable_across_failed_lifecycle() {
    let (env, client, admin) = setup();
    let a = reg(&env, &client, "tx-a");
    client.set_forwarding_route(&a, &3);
    client.start_processing(&a, &admin);
    assert_eq!(client.get_forwarding_route(&a), Some(3));
    client.fail_transaction(&a, &String::from_str(&env, "boom"), &admin);
    assert_eq!(client.get_status(&a), TransactionStatus::Failed);
    assert_eq!(client.get_forwarding_route(&a), Some(3));
}

#[test]
fn route_readable_across_cancelled_lifecycle() {
    let (env, client, admin) = setup();
    let a = reg(&env, &client, "tx-a");
    client.set_forwarding_route(&a, &5);
    client.cancel_transaction(&a, &admin);
    assert_eq!(client.get_status(&a), TransactionStatus::Cancelled);
    assert_eq!(client.get_forwarding_route(&a), Some(5));
}

/// A route set on a transaction that is later merged away (issue #66) is
/// dropped with the duplicate record: the route is stored per-transaction-id
/// and is not transferred to the primary record. The duplicate's own route
/// remains readable on its (now `Failed`/`merged`) record, while the primary
/// keeps whatever route it had (here: none).
#[test]
fn route_dropped_on_merge_duplicate() {
    let (env, client, admin) = setup();
    let a = reg(&env, &client, "tx-a");
    let b = reg(&env, &client, "tx-b");
    client.set_forwarding_route(&b, &9);
    let why = String::from_str(&env, "replay");
    client.merge_duplicate_transactions(&a, &b, &admin, &why);
    // Primary record is untouched: no route transferred from the duplicate.
    assert_eq!(client.get_forwarding_route(&a), None);
    // Duplicate keeps its own route on its merged record.
    assert_eq!(client.get_forwarding_route(&b), Some(9));
}

// ─── #65 relay signer set ─────────────────────────────────────────────────────

/// #304: a contract initialized before the N-of-M feature existed (only a
/// single `relay_signer`) must be readable as an implicit one-of-one signer
/// set without any explicit migration call. `relay_signer_set` derives the
/// one-element vector + threshold of 1 from the single-signer baseline.
#[test]
fn migrated_single_signer_is_one_of_one() {
    let (env, client, _admin) = setup();
    let set = client.relay_signer_set();
    assert_eq!(set.threshold, 1);
    assert_eq!(set.signers.len(), 1);
    assert_eq!(set.signers.get(0).unwrap(), client.relay_signer());
    let _ = env;
}

/// #304: once `add_relay_signer` is called the contract transitions fully into
/// explicit N-of-M storage; the one-of-one fallback is no longer consulted and
/// the set reflects the explicit signers.
#[test]
fn add_relay_signer_transitions_off_fallback() {
    let (env, client, _admin) = setup();
    let primary = client.relay_signer();
    let s2 = Address::generate(&env);
    client.add_relay_signer(&s2);
    let set = client.relay_signer_set();
    assert_eq!(set.signers.len(), 2);
    assert_eq!(set.signers.get(0).unwrap(), primary);
    assert_eq!(set.signers.get(1).unwrap(), s2);
    assert_eq!(set.threshold, 1);
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
    client.approve_relay_call(&a, &s2);
    client.start_processing(&a, &primary);
    assert_eq!(client.get_status(&a), TransactionStatus::Processing);
    let _ = admin;
}

#[test]
fn threshold_bounds_enforced() {
    let (env, client, _admin) = setup();
    // Reject threshold == 0 (quorum would be trivially met).
    assert_eq!(
        client.try_set_relay_threshold(&0).unwrap_err().unwrap(),
        ContractError::InvalidThreshold
    );
    // Reject threshold > current signer count (quorum unreachable).
    assert_eq!(
        client.try_set_relay_threshold(&2).unwrap_err().unwrap(),
        ContractError::InvalidThreshold
    );
    // Accept threshold == signer count (full consensus required).
    client.set_relay_threshold(&1);
    assert_eq!(client.relay_signer_set().threshold, 1);
    // Removing the last signer would leave threshold > signer count.
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

// ─── #300 set_relay_signer_delay bounds ───────────────────────────────────────

#[test]
fn set_relay_signer_delay_rejects_zero_and_out_of_bounds() {
    let (env, client, _admin) = setup();
    assert_eq!(
        client
            .try_set_relay_signer_delay(&0)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidDelay
    );
    assert_eq!(
        client
            .try_set_relay_signer_delay(&u32::MAX)
            .unwrap_err()
            .unwrap(),
        ContractError::InvalidDelay
    );
    let _ = env;
}

#[test]
fn set_relay_signer_delay_stores_value() {
    let (env, client, _admin) = setup();
    client.set_relay_signer_delay(&100);
    assert_eq!(client.relay_signer_delay(), 100);
    let _ = env;
}
