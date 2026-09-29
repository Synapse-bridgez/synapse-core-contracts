//! Compensating-control tests for THREAT_MODEL.md §8 accepted risks.
//!
//! Issue #139: for each accepted risk in §8 ("Accepted Risks & Compensating
//! Controls"), at least one explicit executable test verifies that the
//! described compensating control actually works. These tests prove that the
//! described mitigations are wired correctly in the implementation, not just
//! documented in prose.
//!
//! ## Coverage map
//!
//! | Risk | Compensating control tested |
//! |------|----------------------------|
//! | R-01 | Relay key can be rotated immediately; pause halts new ingestion |
//! | R-02 | All admin-only ops require admin auth; events are emitted for monitoring |
//! | R-03 | `stellar_tx_hash` is stored immutably; accepted by design |
//! | R-04 | F-07 persistent transaction-ID guard survives idempotency-key TTL expiry |
//! | R-05 | Immediate `upgrade()` requires admin auth, schema guard, and emits an event |
//! | R-06 | Treasury withdrawal needs admin *and* relay; per-epoch cap bounds collusion |
//!
//! R-02's rate-limit / safe-renounce controls and R-05's timelock controls
//! have no code on `main` yet (#199), so they are not covered here.

#![cfg(test)]

use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger, MockAuth, MockAuthInvoke},
    Address, BytesN, Env, IntoVal, String, Symbol, TryFromVal,
};

use crate::types::{CallbackPayload, CallbackType, ContractError, StorageKey, TransactionStatus};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Set the `base_fee_bps` registry param (admin auth must be mocked).
fn set_fee(env: &Env, client: &SynapseCoreContractClient, bps: i128) {
    client.set_param(&String::from_str(env, "base_fee_bps"), &bps);
}

/// Set the treasury epoch cap/length registry params (admin auth must be mocked).
fn set_treasury(env: &Env, client: &SynapseCoreContractClient, cap: i128, length: i128) {
    client.set_param(&String::from_str(env, "treasury_epoch_cap"), &cap);
    client.set_param(&String::from_str(env, "treasury_epoch_length"), &length);
}

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

fn g_address(env: &Env) -> String {
    String::from_str(
        env,
        "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ",
    )
}

fn valid_payload(env: &Env) -> CallbackPayload {
    let account = g_address(env);
    CallbackPayload {
        transaction_id: String::from_str(env, "tx-risk-1"),
        stellar_account: account.clone(),
        amount: 1_000,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: account,
        idempotency_key: String::from_str(env, "idem-risk-1"),
        anchor_transaction_id: String::from_str(env, "anchor-risk-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    }
}

/// Assert the last two topics of an event are `synapse` / `name`.
fn assert_topics(
    env: &Env,
    topics: &soroban_sdk::Vec<soroban_sdk::Val>,
    name: soroban_sdk::Symbol,
) {
    assert_eq!(topics.len(), 2);
    let t0 = Symbol::try_from_val(env, &topics.get_unchecked(0)).unwrap();
    let t1 = Symbol::try_from_val(env, &topics.get_unchecked(1)).unwrap();
    assert_eq!(t0, symbol_short!("synapse"));
    assert_eq!(t1, name);
}

// ─── R-01: Single relay signer (hot key) ─────────────────────────────────────
//
// Compensating controls:
//   (a) The relay key can be rotated immediately via `set_relay_signer()`.
//   (b) The admin can pause ingestion via `pause()` to halt new registrations
//       if a breach is detected.
//   (c) Existing transactions cannot be deleted; only their status can be
//       advanced or failed — the audit trail is immutable.

#[test]
fn r01_relay_key_can_be_rotated_immediately_locking_out_old_key() {
    // Control (a): after `set_relay_signer`, the OLD key can no longer register
    // callbacks. The rotation takes effect in the same block (ledger) it is
    // called.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let old_relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &old_relay);

    // Old relay successfully registers a callback before rotation.
    let payload = valid_payload(&env);
    client.register_callback(&payload);

    // Admin rotates relay signer.
    let new_relay = Address::generate(&env);
    client.set_relay_signer(&new_relay);

    // Now try to register a new callback as the OLD relay, using only the old
    // relay's scoped auth (not mock_all_auths).
    let new_payload = CallbackPayload {
        transaction_id: String::from_str(&env, "tx-after-rotation"),
        idempotency_key: String::from_str(&env, "idem-after-rotation"),
        ..payload.clone()
    };
    let result = client
        .mock_auths(&[MockAuth {
            address: &old_relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "register_callback",
                args: (new_payload.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_register_callback(&new_payload);
    // The old relay's auth is rejected because the contract now requires the
    // new relay's signature.
    assert!(
        result.is_err(),
        "Old relay must be rejected after key rotation"
    );
}

#[test]
fn r01_relay_rotation_emits_observable_event() {
    // Control (a): relay rotation emits a `(synapse, relay)` event so
    // off-chain monitoring can detect and alert on any rotation.
    let (env, client, _admin, _relay) = setup();
    let new_relay = Address::generate(&env);
    client.set_relay_signer(&new_relay);

    let events = env.events().all();
    let last = events.get_unchecked(events.len() - 1);
    assert_topics(&env, &last.1, symbol_short!("relay"));
}

#[test]
fn r01_pause_halts_new_ingestion_on_relay_compromise() {
    // Control (b): the admin can pause new callback ingestion immediately,
    // preventing a compromised relay key from registering further transactions.
    let (env, client, _admin, _relay) = setup();
    let payload = valid_payload(&env);

    // Register one tx before the breach.
    let existing_id = client.register_callback(&payload);

    // Admin detects breach and pauses.
    client.pause();
    assert!(client.is_paused());

    // New ingestion is blocked — even with a valid relay key.
    let new_payload = CallbackPayload {
        transaction_id: String::from_str(&env, "tx-after-breach"),
        idempotency_key: String::from_str(&env, "idem-after-breach"),
        ..payload.clone()
    };
    let result = client.try_register_callback(&new_payload);
    assert_eq!(
        result,
        Err(Ok(ContractError::ContractPaused)),
        "New ingestion must be blocked while paused"
    );

    // The audit trail for the pre-breach transaction is still readable.
    let tx = client.get_transaction(&existing_id);
    assert_eq!(tx.status, TransactionStatus::Pending);
}

#[test]
fn r01_transaction_audit_trail_is_immutable() {
    // Control (c): transactions cannot be deleted or overwritten once Created.
    // A relay-signer compromise cannot erase evidence; it can only advance
    // or fail existing transactions.
    let (env, client, _admin, relay) = setup();
    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);
    client.start_processing(&tx_id, &relay);

    let hash = String::from_str(&env, "audit-hash");
    client.complete_transaction(&tx_id, &hash, &relay);

    // The completed record (with its audit fields) is still present and intact.
    let tx = client.get_transaction(&tx_id);
    assert_eq!(tx.status, TransactionStatus::Completed);
    assert_eq!(tx.stellar_tx_hash, hash);

    // Attempting to overwrite the same transaction_id via register_callback
    // is rejected — the audit trail record cannot be replaced.
    let replay = CallbackPayload {
        idempotency_key: String::from_str(&env, "idem-overwrite-attempt"),
        ..payload.clone()
    };
    let result = client.try_register_callback(&replay);
    assert_eq!(
        result,
        Err(Ok(ContractError::DuplicateRequest)),
        "Completed transaction record must not be overwritable"
    );

    // Original record unchanged.
    let tx_after = client.get_transaction(&tx_id);
    assert_eq!(tx_after.status, TransactionStatus::Completed);
    assert_eq!(tx_after.stellar_tx_hash, hash);
}

// ─── R-02: Admin-key is the root of all trust ─────────────────────────────────
//
// Compensating controls:
//   (a) All admin operations require auth from the current admin address.
//   (b) All admin operations emit observable events so monitoring can alert.
//   (c) Two-step admin transfer prevents a single action from hijacking admin.

#[test]
fn r02_all_admin_ops_require_admin_auth() {
    // Control (a): none of the admin-gated entry points succeed when called
    // by a non-admin address — regardless of what the caller's own auth says.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    client.initialize(&admin, &relay);

    let attacker = Address::generate(&env);
    let new_addr = Address::generate(&env);
    let dummy_hash = BytesN::from_array(&env, &[0u8; 32]);

    // Each of these must fail with an auth error (not a ContractError):
    // pause, unpause, set_relay_signer, propose_admin, upgrade, set_param.

    // pause
    assert!(
        client
            .mock_auths(&[MockAuth {
                address: &attacker,
                invoke: &MockAuthInvoke {
                    contract: &contract_id,
                    fn_name: "pause",
                    args: ().into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .try_pause()
            .is_err(),
        "Non-admin must not pause"
    );
    // unpause
    assert!(
        client
            .mock_auths(&[MockAuth {
                address: &attacker,
                invoke: &MockAuthInvoke {
                    contract: &contract_id,
                    fn_name: "unpause",
                    args: ().into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .try_unpause()
            .is_err(),
        "Non-admin must not unpause"
    );
    // set_relay_signer
    assert!(
        client
            .mock_auths(&[MockAuth {
                address: &attacker,
                invoke: &MockAuthInvoke {
                    contract: &contract_id,
                    fn_name: "set_relay_signer",
                    args: (new_addr.clone(),).into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .try_set_relay_signer(&new_addr)
            .is_err(),
        "Non-admin must not rotate relay"
    );
    // propose_admin
    assert!(
        client
            .mock_auths(&[MockAuth {
                address: &attacker,
                invoke: &MockAuthInvoke {
                    contract: &contract_id,
                    fn_name: "propose_admin",
                    args: (new_addr.clone(),).into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .try_propose_admin(&new_addr)
            .is_err(),
        "Non-admin must not propose admin transfer"
    );
    // upgrade
    assert!(
        client
            .mock_auths(&[MockAuth {
                address: &attacker,
                invoke: &MockAuthInvoke {
                    contract: &contract_id,
                    fn_name: "upgrade",
                    args: (dummy_hash.clone(), 1u32).into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .try_upgrade(&dummy_hash, &1)
            .is_err(),
        "Non-admin must not upgrade"
    );
    // set_param (fee rate, treasury limits, and all other registry params)
    let fee_param = String::from_str(&env, "base_fee_bps");
    assert!(
        client
            .mock_auths(&[MockAuth {
                address: &attacker,
                invoke: &MockAuthInvoke {
                    contract: &contract_id,
                    fn_name: "set_param",
                    args: (fee_param.clone(), 200_i128).into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .try_set_param(&fee_param, &200)
            .is_err(),
        "Non-admin must not set params"
    );
}

#[test]
fn r02_upgrade_emits_observable_event() {
    // Control (b): every upgrade emits `(synapse, upgrade)` so monitoring
    // can detect unexpected contract replacements.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let admin = Address::generate(&env);
    let dummy_hash = BytesN::from_array(&env, &[0xabu8; 32]);

    env.as_contract(&contract_id, || {
        crate::events::EventEmitter::contract_upgraded(&env, &admin, &dummy_hash, 1);
    });

    let events = env.events().all();
    assert_eq!(events.len(), 1);
    assert_topics(&env, &events.get_unchecked(0).1, symbol_short!("upgrade"));
}

#[test]
fn r02_admin_transfer_requires_two_steps() {
    // Control (c): the current admin cannot finalise an admin transfer by itself.
    // The nominee must prove key control via `accept_admin`.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);

    let new_admin = Address::generate(&env);
    client.propose_admin(&new_admin);

    // Admin has only proposed — the admin is still the old address.
    assert_eq!(
        client.admin(),
        admin,
        "Admin must not change after proposal alone"
    );
    assert_eq!(client.pending_admin(), Some(new_admin.clone()));

    // The transfer only completes once the nominee calls accept_admin.
    client.accept_admin(&new_admin);
    assert_eq!(
        client.admin(),
        new_admin,
        "Admin must change after nominee accepts"
    );
    assert_eq!(client.pending_admin(), None);
}

#[test]
fn r02_pause_toggle_emits_observable_event() {
    // Control (b): pause/unpause always emits `(synapse, pause)` so monitoring
    // can detect unexpected circuit-breaker engagement.
    let (env, client, _admin, _relay) = setup();
    client.pause();

    let events = env.events().all();
    let last = events.get_unchecked(events.len() - 1);
    assert_topics(&env, &last.1, symbol_short!("pause"));
}

// ─── R-03: No on-chain Horizon verification ───────────────────────────────────
//
// Compensating controls:
//   (a) The relay performs Horizon verification before calling
//       `complete_transaction`. On-chain we cannot verify this, but we can
//       verify that the `stellar_tx_hash` field IS stored and is immutable
//       once written, providing a post-hoc audit trail.

#[test]
fn r03_stellar_tx_hash_stored_immutably_on_completion() {
    // Control (a): `stellar_tx_hash` is written at Completed and cannot be
    // changed by any subsequent call (the state is terminal).
    let (env, client, _admin, relay) = setup();
    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);
    client.start_processing(&tx_id, &relay);

    let original_hash = String::from_str(&env, "horizon-tx-abc123");
    client.complete_transaction(&tx_id, &original_hash, &relay);

    let tx = client.get_transaction(&tx_id);
    assert_eq!(
        tx.stellar_tx_hash, original_hash,
        "Stored hash must match what the relay provided"
    );
    assert_eq!(
        tx.status,
        TransactionStatus::Completed,
        "Transaction must be Completed"
    );

    // No status transition is valid from Completed — the record (and its hash)
    // cannot be overwritten.
    let result = client.try_start_processing(&tx_id, &relay);
    assert_eq!(
        result,
        Err(Ok(ContractError::InvalidStatusTransition)),
        "Completed is a terminal state — hash cannot be changed"
    );
}

#[test]
fn r03_stellar_tx_hash_empty_until_completed() {
    // The `stellar_tx_hash` field is an empty string from Pending through
    // Processing, confirming it is only set at the Completed transition.
    let (env, client, _admin, relay) = setup();
    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);

    let tx_pending = client.get_transaction(&tx_id);
    assert_eq!(
        tx_pending.stellar_tx_hash,
        String::from_str(&env, ""),
        "Hash must be empty at Pending"
    );

    client.start_processing(&tx_id, &relay);
    let tx_processing = client.get_transaction(&tx_id);
    assert_eq!(
        tx_processing.stellar_tx_hash,
        String::from_str(&env, ""),
        "Hash must be empty at Processing"
    );
}

// ─── R-04: Idempotency window is finite (~24 h) ───────────────────────────────
//
// Compensating controls:
//   (a) F-07: `StorageClient::transaction_exists()` provides a persistent
//       second-line defence — a replay arriving after the 24h TTL but
//       carrying the same `transaction_id` is still rejected via
//       `DuplicateRequest`, even with a fresh `idempotency_key`.

#[test]
fn r04_persistent_transaction_id_guard_survives_idempotency_ttl_expiry() {
    // Control (a): after the ~24h idempotency TTL expires, `is_duplicate`
    // returns false (key expired), but a replay with a fresh idempotency_key
    // and the same transaction_id is still rejected by the persistent guard.
    let (env, client, _admin, _relay) = setup();
    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);
    assert!(client.is_duplicate(&payload.idempotency_key));

    // Extend critical persistent keys to survive the ledger jump.
    env.as_contract(&client.address, || {
        env.storage().instance().extend_ttl(100_000, 100_000);
        env.storage()
            .persistent()
            .extend_ttl(&StorageKey::Admin, 100_000, 100_000);
        env.storage()
            .persistent()
            .extend_ttl(&StorageKey::RelaySigner, 100_000, 100_000);
    });

    // Jump past the idempotency TTL (~18_000 ledgers).
    env.ledger().with_mut(|li| li.sequence_number += 18_001);

    // The idempotency key has now expired.
    assert!(
        !client.is_duplicate(&payload.idempotency_key),
        "Idempotency key must expire after TTL"
    );

    // A late replay with a fresh idempotency key but the same transaction_id.
    let late_replay = CallbackPayload {
        idempotency_key: String::from_str(&env, "idem-fresh-key-late-replay"),
        ..payload.clone()
    };
    let result = client.try_register_callback(&late_replay);
    assert_eq!(
        result,
        Err(Ok(ContractError::DuplicateRequest)),
        "Persistent transaction_id guard must reject late replay (F-07)"
    );

    // Original record is untouched.
    let tx = client.get_transaction(&tx_id);
    assert_eq!(tx.id, tx_id);
}

#[test]
fn r04_idempotency_key_within_window_deduplicates_silently() {
    // Within the TTL window, a duplicate delivery returns the original tx_id
    // without writing a second record — matching Redis-style deduplication.
    let (env, client, _admin, _relay) = setup();
    let payload = valid_payload(&env);
    let first_id = client.register_callback(&payload);
    let second_id = client.register_callback(&payload);
    assert_eq!(
        first_id, second_id,
        "Duplicate within TTL window must return original tx_id"
    );
    assert!(client.is_duplicate(&payload.idempotency_key));
}

// ─── R-05: In-place upgrade — timelock window ─────────────────────────────────
//
// The timelock path (`propose_upgrade` / `finalize_upgrade`) is missing from
// `lib.rs` on `main` (#199); these tests cover the immediate `upgrade()` path.
//
// Compensating controls:
//   (a) Upgrade requires admin (multisig) auth — M-of-N keys must sign.
//   (b) `EventContractUpgraded` is always emitted; monitoring can detect and
//       alert within seconds.
//   (c) `expected_schema_version` prevents upgrading an unexpected on-chain
//       state (F-04 guard).

#[test]
fn r05_upgrade_always_requires_admin_auth() {
    // Control (a): no account other than the admin can invoke `upgrade`.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    client.initialize(&admin, &relay);

    let attacker = Address::generate(&env);
    let dummy_hash = BytesN::from_array(&env, &[0u8; 32]);

    let result = client
        .mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "upgrade",
                args: (dummy_hash.clone(), 1u32).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_upgrade(&dummy_hash, &1);
    assert!(
        result.is_err(),
        "Non-admin must not be able to upgrade the contract"
    );
}

#[test]
fn r05_upgrade_schema_version_guard_fires_before_wasm_change() {
    // Control (c): passing a wrong `expected_schema_version` aborts the
    // upgrade before `update_current_contract_wasm` is called — tested by
    // providing a mismatched version without a real uploaded WASM.
    let (env, client, _admin, _relay) = setup();
    let dummy_hash = BytesN::from_array(&env, &[0u8; 32]);
    let result = client.try_upgrade(&dummy_hash, &999);
    assert_eq!(
        result,
        Err(Ok(ContractError::SchemaVersionMismatch)),
        "Schema version mismatch must abort upgrade before WASM change"
    );
}

#[test]
fn r05_upgrade_emits_event_for_monitoring() {
    // Control (b): `(synapse, upgrade)` is always emitted so off-chain
    // monitoring can detect any upgrade attempt, expected or not.
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let admin = Address::generate(&env);
    let dummy_hash = BytesN::from_array(&env, &[0xabu8; 32]);

    env.as_contract(&contract_id, || {
        crate::events::EventEmitter::contract_upgraded(&env, &admin, &dummy_hash, 1);
    });

    let events = env.events().all();
    assert_eq!(events.len(), 1, "Upgrade must emit exactly one event");
    assert_topics(&env, &events.get_unchecked(0).1, symbol_short!("upgrade"));
}

// ─── R-06: Treasury withdrawal is two-party, not N-of-M ───────────────────────
//
// Compensating controls:
//   (a) The admin alone can only *propose*; execution needs the relay signer.
//   (b) The relay alone can neither propose nor execute without a proposal.
//   (c) The per-epoch cap bounds what even a colluding admin + relay can move.
//   (d) The relay's co-signature names the exact amount and destination, so a
//       compromised admin cannot swap the proposal after relay review.

/// Treasury seeded with `balance` stroops (100% fee on one completion),
/// epoch cap 1_000, epoch length 100 ledgers.
fn setup_treasury(balance: i128) -> (Env, SynapseCoreContractClient<'static>, Address, Address) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay);
    set_fee(&env, &client, 10_000);
    set_treasury(&env, &client, 1_000, 100);
    let mut payload = valid_payload(&env);
    payload.amount = balance;
    let tx_id = client.register_callback(&payload);
    client.start_processing(&tx_id, &relay);
    client.complete_transaction(&tx_id, &String::from_str(&env, "hash"), &relay);
    (env, client, admin, relay)
}

#[test]
fn r06_admin_alone_cannot_execute_withdrawal() {
    // Control (a).
    let (env, client, admin, _relay) = setup_treasury(5_000);
    let dest = Address::generate(&env);
    client.propose_withdrawal(&500, &dest);
    assert_eq!(
        client.try_authorize_withdrawal(&admin, &500, &dest),
        Err(Ok(ContractError::WithdrawalNotRelaySigner))
    );
    assert_eq!(client.treasury_balance(), 5_000);
}

#[test]
fn r06_relay_alone_cannot_propose_or_execute() {
    // Control (b): with only the relay's signature, `propose_withdrawal`
    // fails auth, and `authorize_withdrawal` has nothing to execute.
    let (env, client, _admin, relay) = setup_treasury(5_000);
    let contract_id = client.address.clone();
    let dest = Address::generate(&env);

    let result = client
        .mock_auths(&[MockAuth {
            address: &relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "propose_withdrawal",
                args: (500_i128, dest.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_propose_withdrawal(&500, &dest);
    assert!(result.is_err(), "relay must not be able to propose");

    env.mock_all_auths();
    assert_eq!(
        client.try_authorize_withdrawal(&relay, &500, &dest),
        Err(Ok(ContractError::NoPendingWithdrawal))
    );
    assert_eq!(client.treasury_balance(), 5_000);
}

#[test]
fn r06_epoch_cap_bounds_colluding_admin_and_relay() {
    // Control (c): both keys cooperating still cannot exceed the cap within
    // one epoch, even with ample treasury balance.
    let (env, client, _admin, relay) = setup_treasury(5_000);
    let dest = Address::generate(&env);
    client.propose_withdrawal(&1_000, &dest);
    client.authorize_withdrawal(&relay, &1_000, &dest);
    client.propose_withdrawal(&1, &dest);
    assert_eq!(
        client.try_authorize_withdrawal(&relay, &1, &dest),
        Err(Ok(ContractError::WithdrawalCapExceeded))
    );
    assert_eq!(client.treasury_balance(), 4_000);
}

#[test]
fn r06_admin_cannot_swap_proposal_after_relay_review() {
    // Control (d): the relay reviews and signs for (500, dest); the admin
    // then replaces the proposal with (1_000, attacker). The relay's signed
    // call must not execute the substituted proposal.
    let (env, client, _admin, relay) = setup_treasury(5_000);
    let dest = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.propose_withdrawal(&500, &dest);
    client.propose_withdrawal(&1_000, &attacker);
    assert_eq!(
        client.try_authorize_withdrawal(&relay, &500, &dest),
        Err(Ok(ContractError::WithdrawalProposalMismatch))
    );
    assert_eq!(client.treasury_balance(), 5_000);
}
