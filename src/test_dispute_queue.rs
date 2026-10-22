//! Integration tests for the dispute queue (issue #80) interacting with the
//! merge mechanism (issue #66 / #302).
//!
//! Rule adopted in #302: merged-away transactions are excluded from every
//! status-based bulk query (`get_transactions_by_status`, `get_dispute_queue`)
//! but remain directly queryable by `tx_id` (and via `get_merged_into`).

use super::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

fn setup() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, PaymentContract);
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let client = PaymentContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    (env, contract_id, admin, merchant)
}

#[test]
fn test_dispute_queue_excludes_merged_away_transaction() {
    let (env, contract_id, _admin, merchant) = setup();
    let client = PaymentContractClient::new(&env, &contract_id);

    let primary = client.create_transaction(&merchant, &1_000_i128, &None);
    let duplicate = client.create_transaction(&merchant, &1_000_i128, &None);

    // Dispute the duplicate, then discover it is a duplicate and merge it away.
    client.dispute_transaction(&duplicate);
    client.merge_duplicate_transactions(&primary, &duplicate);

    // The merged-away transaction must not appear in the dispute queue.
    let queue = client.get_dispute_queue();
    assert!(!queue.contains(&duplicate));

    // It must not appear in status-based bulk queries either.
    let disputed = client.get_transactions_by_status(&TransactionStatus::Disputed);
    assert!(!disputed.contains(&duplicate));

    // But it remains directly queryable by tx_id and round-trips via get_merged_into.
    let stored = client.get_transaction(&duplicate);
    assert_eq!(stored.id, duplicate);
    assert_eq!(client.get_merged_into(&duplicate), Some(primary));
}

#[test]
fn test_dispute_queue_keeps_primary_after_merge() {
    let (env, contract_id, _admin, merchant) = setup();
    let client = PaymentContractClient::new(&env, &contract_id);

    let primary = client.create_transaction(&merchant, &500_i128, &None);
    let duplicate = client.create_transaction(&merchant, &500_i128, &None);

    client.dispute_transaction(&primary);
    client.merge_duplicate_transactions(&primary, &duplicate);

    // The primary is still disputed and must remain in the queue exactly once.
    let queue = client.get_dispute_queue();
    let mut count = 0u32;
    for id in queue.iter() {
        if id == primary {
            count += 1;
        }
        assert_ne!(id, duplicate);
    }
    assert_eq!(count, 1);
}
