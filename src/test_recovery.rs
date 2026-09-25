//! Tests for the `merge_duplicate_transactions` recovery entry point (#63).

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, String};

use crate::types::{CallbackPayload, CallbackType, ContractError, TransactionStatus};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

fn setup() -> (Env, SynapseCoreContractClient<'static>, Address) {
    let env = Env::default();
    let id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
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
