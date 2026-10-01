//! Tests for the hot-path allocation changes (#121).
//!
//! * Fixed-capacity stack buffers in `validation.rs` (`[u8; 56]` strkey,
//!   `[u8; 12]` asset code) at, one below, and one above their capacity —
//!   the case where a capacity/cap mismatch would truncate or panic.
//! * Keys encoded once via `storage::encode_key` address exactly the same
//!   ledger entries as the plain `StorageKey` form.
//! * The shared empty `String` in `register_callback`.
//! * Relay-first `assert_is_relay_or_admin` keeps every outcome unchanged.

#![cfg(test)]

extern crate std;

use soroban_sdk::{
    testutils::{
        storage::{Persistent as _, Temporary as _},
        Address as _,
    },
    Address, Env, String,
};

use crate::types::{CallbackPayload, CallbackType, ContractError, StorageKey, TransactionStatus};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

/// Checksum-valid SEP-23 G-address (56 chars).
const G_VALID: &str = "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ";

fn setup() -> (Env, SynapseCoreContractClient<'static>, Address, Address) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    (env, client, admin, relay)
}

fn payload(env: &Env, n: u32, account: &str, issuer: &str, asset_code: &str) -> CallbackPayload {
    let id = std::format!("tx-{n}");
    let idem = std::format!("idem-{n}");
    CallbackPayload {
        transaction_id: String::from_str(env, &id),
        stellar_account: String::from_str(env, account),
        amount: 1_000,
        asset_code: String::from_str(env, asset_code),
        asset_issuer: String::from_str(env, issuer),
        idempotency_key: String::from_str(env, &idem),
        anchor_transaction_id: String::from_str(env, "anchor-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    }
}

// ─── Fixed-capacity buffer boundaries ────────────────────────────────────────

#[test]
fn strkey_buffer_boundary_at_below_and_above_capacity() {
    let (env, client, _admin, _relay) = setup();
    let below = &G_VALID[..55];
    let above = std::format!("{G_VALID}A");

    for (n, value, ok) in [
        (1, below, false),
        (2, G_VALID, true),
        (3, above.as_str(), false),
    ] {
        let as_account = payload(&env, n, value, G_VALID, "USDC");
        let as_issuer = payload(&env, n + 10, G_VALID, value, "USDC");
        if ok {
            assert!(client.try_register_callback(&as_account).is_ok());
            assert!(client.try_register_callback(&as_issuer).is_ok());
        } else {
            assert_eq!(
                client.try_register_callback(&as_account),
                Err(Ok(ContractError::InvalidStellarAccount)),
                "account len {}",
                value.len()
            );
            assert_eq!(
                client.try_register_callback(&as_issuer),
                Err(Ok(ContractError::InvalidAssetIssuer)),
                "issuer len {}",
                value.len()
            );
        }
    }
}

#[test]
fn asset_code_buffer_boundary_at_below_and_above_capacity() {
    let (env, client, _admin, _relay) = setup();
    let cases = [
        (1, "", false),
        (2, "ABCDEFGHIJK", true),    // 11 — one below
        (3, "ABCDEFGHIJKL", true),   // 12 — exactly at capacity
        (4, "ABCDEFGHIJKLM", false), // 13 — one above
    ];
    for (n, code, ok) in cases {
        let result = client.try_register_callback(&payload(&env, n, G_VALID, G_VALID, code));
        if ok {
            assert!(result.is_ok(), "asset code len {}", code.len());
        } else {
            assert_eq!(
                result,
                Err(Ok(ContractError::InvalidAssetCode)),
                "asset code len {}",
                code.len()
            );
        }
    }
    // The full 12 bytes are checked, not a truncated prefix.
    let lower_last = payload(&env, 5, G_VALID, G_VALID, "ABCDEFGHIJKl");
    assert_eq!(
        client.try_register_callback(&lower_last),
        Err(Ok(ContractError::InvalidAssetCode))
    );
}

// ─── Encoded-once storage keys ───────────────────────────────────────────────

#[test]
fn encoded_keys_address_the_same_ledger_entries() {
    let (env, client, _admin, relay) = setup();
    let p = payload(&env, 1, G_VALID, G_VALID, "USDC");
    let tx_id = client.register_callback(&p);
    client.start_processing(&tx_id, &relay);

    env.as_contract(&client.address, || {
        let tx_key = StorageKey::Transaction(tx_id.clone());
        let idem_key = StorageKey::IdempotencyKey(p.idempotency_key.clone());

        // Written via encode_key, visible via the plain StorageKey.
        let stored: crate::types::Transaction = env.storage().persistent().get(&tx_key).unwrap();
        assert_eq!(stored.status, TransactionStatus::Processing);
        assert!(env.storage().temporary().has(&idem_key));

        // extend_ttl hit the same entries as set().
        assert!(env.storage().persistent().get_ttl(&tx_key) >= 100_000);
        assert!(env.storage().temporary().get_ttl(&idem_key) >= 18_000);
    });

    // Idempotency lookup (plain StorageKey) still sees the encoded write.
    assert!(client.is_duplicate(&p.idempotency_key));
    assert_eq!(client.register_callback(&p), tx_id);
}

// ─── Shared empty string ─────────────────────────────────────────────────────

#[test]
fn shared_empty_string_fields_stay_independent() {
    let (env, client, _admin, relay) = setup();
    let tx_id = client.register_callback(&payload(&env, 1, G_VALID, G_VALID, "USDC"));
    let empty = String::from_str(&env, "");

    let tx = client.get_transaction(&tx_id);
    assert_eq!(tx.stellar_tx_hash, empty);
    assert_eq!(tx.failure_reason, empty);

    client.start_processing(&tx_id, &relay);
    let hash = String::from_str(&env, "abc123");
    client.complete_transaction(&tx_id, &hash, &relay);
    let tx = client.get_transaction(&tx_id);
    assert_eq!(tx.stellar_tx_hash, hash);
    assert_eq!(tx.failure_reason, empty);
}

// ─── Relay-first role check ──────────────────────────────────────────────────

#[test]
fn relay_first_role_check_keeps_outcomes() {
    let (env, client, admin, relay) = setup();
    let a = client.register_callback(&payload(&env, 1, G_VALID, G_VALID, "USDC"));
    let b = client.register_callback(&payload(&env, 2, G_VALID, G_VALID, "USDC"));
    let c = client.register_callback(&payload(&env, 3, G_VALID, G_VALID, "USDC"));

    client.start_processing(&a, &relay);
    client.start_processing(&b, &admin);
    assert_eq!(
        client.try_start_processing(&c, &Address::generate(&env)),
        Err(Ok(ContractError::Unauthorised))
    );
    assert_eq!(client.get_status(&a), TransactionStatus::Processing);
    assert_eq!(client.get_status(&b), TransactionStatus::Processing);
    assert_eq!(client.get_status(&c), TransactionStatus::Pending);
}

#[test]
fn relay_first_role_check_before_initialise_is_not_initialised() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let tx_id = String::from_str(&env, "tx-1");
    assert_eq!(
        client.try_start_processing(&tx_id, &Address::generate(&env)),
        Err(Ok(ContractError::NotInitialised))
    );
}
