//! # Adversarial Authorization Test Suite — Issue #130
//!
//! Systematically covers every privileged entry point with deliberate adversarial
//! scenarios: wrong-caller, forged/mismatched signers, cross-role confusion, and
//! replay attempts. One section per entry point, so coverage gaps are visually
//! obvious in code review.
//!
//! ## `THREAT_MODEL.md` cross-references
//!
//! | Section in THREAT_MODEL.md          | Tests here                                     |
//! |--------------------------------------|------------------------------------------------|
//! | §4.1 `initialize` — double-init      | `auth_initialize_*`                            |
//! | §4.2 `register_callback` — unauth    | `auth_register_callback_*`                     |
//! | §4.3 status transitions — caller id  | `auth_start_processing_*`, `auth_complete_*`,  |
//! |                                      | `auth_fail_*`                                  |
//! | §4.4 `propose_admin` / `accept_admin`| `auth_propose_admin_*`, `auth_accept_admin_*`  |
//! | §4.4 `set_relay_signer`              | `auth_set_relay_signer_*`                      |
//! | §4.5 `upgrade`                       | `auth_upgrade_*`                               |
//! | §4.6 `pause` / `unpause`             | `auth_pause_*`, `auth_unpause_*`               |
//! | §2   cross-role confusion            | `cross_role_*`                                 |
//! | §4.2 replay / duplicate              | `replay_*`                                     |

#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger, MockAuth, MockAuthInvoke},
    Address, BytesN, Env, IntoVal, String,
};

use crate::types::{CallbackPayload, CallbackType, ContractError, TransactionStatus};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

// ─── Shared helpers ───────────────────────────────────────────────────────────

/// A real SEP-23 ed25519 public-key strkey (CRC16-valid).
fn g_address(env: &Env) -> String {
    String::from_str(
        env,
        "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ",
    )
}

/// Build a clean contract environment with `mock_all_auths` for setup convenience.
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

/// Build a contract environment WITHOUT `mock_all_auths` so each call requires
/// explicit scoped `MockAuth` entries. Used for all adversarial tests.
fn setup_strict() -> (
    Env,
    SynapseCoreContractClient<'static>,
    Address,
    Address,
    soroban_sdk::Address, // contract_id
) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);

    // Initialise in strict mode (explicit MockAuth for each role).
    client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "initialize",
                args: (admin.clone(), relay.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .initialize(&admin, &relay);

    let addr = contract_id.clone();
    (env, client, admin, relay, addr)
}

fn valid_payload(env: &Env) -> CallbackPayload {
    let account = g_address(env);
    CallbackPayload {
        transaction_id: String::from_str(env, "tx-adv-1"),
        stellar_account: account.clone(),
        amount: 5_000,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: account,
        idempotency_key: String::from_str(env, "idem-adv-1"),
        anchor_transaction_id: String::from_str(env, "anchor-adv-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    }
}

fn register_via_relay(
    env: &Env,
    client: &SynapseCoreContractClient,
    contract_id: &Address,
    relay: &Address,
    payload: &CallbackPayload,
) -> String {
    client
        .mock_auths(&[MockAuth {
            address: relay,
            invoke: &MockAuthInvoke {
                contract: contract_id,
                fn_name: "register_callback",
                args: (payload.clone(),).into_val(env),
                sub_invokes: &[],
            },
        }])
        .register_callback(payload)
}

// ═══════════════════════════════════════════════════════════════════════════════
// §1 — initialize()
// THREAT_MODEL.md §4.1
// ═══════════════════════════════════════════════════════════════════════════════

/// [`THREAT_MODEL` §4.1] Double-initialisation must be rejected regardless of
/// caller, preventing an attacker from re-setting admin/relay after deployment.
#[test]
fn auth_initialize_rejects_double_init_from_original_admin() {
    let (_env, client, admin, relay) = setup();
    let result = client.try_initialize(&admin, &relay);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialised)));
}

/// [`THREAT_MODEL` §4.1] Double-initialisation by a completely unrelated attacker
/// must also be rejected (the guard is unconditional, not role-gated).
#[test]
fn auth_initialize_rejects_double_init_from_attacker() {
    let (env, client, _admin, _relay) = setup();
    let attacker = Address::generate(&env);
    let attacker2 = Address::generate(&env);
    // mock_all_auths is active from setup(); double-init rejects before auth.
    let result = client.try_initialize(&attacker, &attacker2);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialised)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// §2 — register_callback()
// THREAT_MODEL.md §4.2
// ═══════════════════════════════════════════════════════════════════════════════

/// [`THREAT_MODEL` §4.2] Only the trusted relay signer may register callbacks.
/// An unrelated attacker supplying its own auth must be rejected at the auth layer.
#[test]
fn auth_register_callback_attacker_auth_rejected() {
    let (env, client, _admin, _relay, contract_id) = setup_strict();
    let attacker = Address::generate(&env);
    let payload = valid_payload(&env);

    let result = client
        .mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "register_callback",
                args: (payload.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_register_callback(&payload);

    // Host auth failure — the relay's `require_auth` was not satisfied.
    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.2 / §2] The admin address cannot register callbacks — admin
/// is not in the relay role; this covers cross-role confusion.
#[test]
fn auth_register_callback_admin_auth_rejected() {
    let (env, client, admin, _relay, contract_id) = setup_strict();
    let payload = valid_payload(&env);

    let result = client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "register_callback",
                args: (payload.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_register_callback(&payload);

    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.2] A forged/mismatched relay signer — a caller claiming to
/// be the relay in the payload metadata but supplying a different key's auth —
/// must be rejected.
#[test]
fn auth_register_callback_forged_relay_address_rejected() {
    let (env, client, _admin, _relay, contract_id) = setup_strict();
    let forged_relay = Address::generate(&env);
    let payload = valid_payload(&env);

    // The forged_relay authorises the call, but the contract's stored relay
    // signer is `relay`. The `relay.require_auth()` check will fail because
    // `forged_relay`'s auth was presented, not `relay`'s.
    let result = client
        .mock_auths(&[MockAuth {
            address: &forged_relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "register_callback",
                args: (payload.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_register_callback(&payload);

    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.2] No auth at all — bare call with no `MockAuth` entries must
/// fail at the host level.
#[test]
fn auth_register_callback_no_auth_rejected() {
    let (env, client, _admin, _relay, _contract_id) = setup_strict();
    let payload = valid_payload(&env);
    // No mock_auths configured — the host rejects the missing auth.
    let result = client.try_register_callback(&payload);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// §3 — start_processing(tx_id, caller)
// THREAT_MODEL.md §4.3
// ═══════════════════════════════════════════════════════════════════════════════

/// [`THREAT_MODEL` §4.3] Unrelated bystander passed as `caller` must be rejected
/// because it is not in the admin/relay set — checks role membership check.
#[test]
fn auth_start_processing_bystander_as_caller_rejected() {
    let (env, client, _admin, _relay) = setup();
    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);

    let bystander = Address::generate(&env);
    let result = client.try_start_processing(&tx_id, &bystander);
    assert_eq!(result, Err(Ok(ContractError::Unauthorised)));
}

/// [`THREAT_MODEL` §4.3] Passing the relay address as `caller` with only the
/// admin's auth supplied must fail — `caller.require_auth()` requires the
/// relay's own signature, not a different role's.
#[test]
fn auth_start_processing_caller_relay_but_admin_auth_supplied_rejected() {
    let (env, client, admin, relay, contract_id) = setup_strict();
    let payload = valid_payload(&env);
    let tx_id = register_via_relay(&env, &client, &contract_id, &relay, &payload);

    // `caller` = relay (passes membership check) but only admin's auth is present.
    let result = client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "start_processing",
                args: (tx_id.clone(), relay.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_start_processing(&tx_id, &relay);

    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.3] Cross-role confusion — the relay passes admin's address
/// as `caller` but supplies the relay's own auth. The membership check rejects
/// because `caller` (admin) ≠ relay, yet `admin.require_auth()` is not called here.
/// Actually, admin IS in the admin/relay set — so this variant tests that
/// `caller.require_auth()` is called for the actual `caller` arg.
#[test]
fn auth_start_processing_caller_admin_with_relay_auth_rejected() {
    let (env, client, admin, relay, contract_id) = setup_strict();
    let payload = valid_payload(&env);
    let tx_id = register_via_relay(&env, &client, &contract_id, &relay, &payload);

    // `caller` = admin (passes membership), auth supplied is for relay.
    // admin.require_auth() is not satisfied → host error.
    let result = client
        .mock_auths(&[MockAuth {
            address: &relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "start_processing",
                args: (tx_id.clone(), admin.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_start_processing(&tx_id, &admin);

    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.3] Correct relay auth, correct caller — must succeed.
#[test]
fn auth_start_processing_correct_relay_auth_succeeds() {
    let (env, client, _admin, relay, contract_id) = setup_strict();
    let payload = valid_payload(&env);
    let tx_id = register_via_relay(&env, &client, &contract_id, &relay, &payload);

    client
        .mock_auths(&[MockAuth {
            address: &relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "start_processing",
                args: (tx_id.clone(), relay.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .start_processing(&tx_id, &relay);

    assert_eq!(client.get_status(&tx_id), TransactionStatus::Processing);
}

/// [`THREAT_MODEL` §4.3] Admin as caller with admin auth — also valid (admin can
/// drive transitions). This is the dual of the relay test above.
#[test]
fn auth_start_processing_admin_caller_with_admin_auth_succeeds() {
    let (env, client, admin, relay, contract_id) = setup_strict();
    let payload = valid_payload(&env);
    let tx_id = register_via_relay(&env, &client, &contract_id, &relay, &payload);

    client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "start_processing",
                args: (tx_id.clone(), admin.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .start_processing(&tx_id, &admin);

    assert_eq!(client.get_status(&tx_id), TransactionStatus::Processing);
}

// ═══════════════════════════════════════════════════════════════════════════════
// §4 — complete_transaction(tx_id, stellar_tx_hash, caller)
// THREAT_MODEL.md §4.3
// ═══════════════════════════════════════════════════════════════════════════════

/// [`THREAT_MODEL` §4.3] Bystander as caller must be rejected before any state write.
#[test]
fn auth_complete_transaction_bystander_rejected() {
    let (env, client, _admin, relay) = setup();
    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);
    client.start_processing(&tx_id, &relay);

    let bystander = Address::generate(&env);
    let hash = String::from_str(&env, "hash-adv");
    let result = client.try_complete_transaction(&tx_id, &hash, &bystander);
    assert_eq!(result, Err(Ok(ContractError::Unauthorised)));
}

/// [`THREAT_MODEL` §4.3] `caller` = relay but only admin auth present — must fail.
#[test]
fn auth_complete_transaction_caller_relay_admin_auth_rejected() {
    let (env, client, admin, relay, contract_id) = setup_strict();
    let payload = valid_payload(&env);
    let tx_id = register_via_relay(&env, &client, &contract_id, &relay, &payload);

    client
        .mock_auths(&[MockAuth {
            address: &relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "start_processing",
                args: (tx_id.clone(), relay.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .start_processing(&tx_id, &relay);

    let hash = String::from_str(&env, "hash-adv-comp");
    let result = client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "complete_transaction",
                args: (tx_id.clone(), hash.clone(), relay.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_complete_transaction(&tx_id, &hash, &relay);

    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.3] Correct relay caller + relay auth — full success path.
#[test]
fn auth_complete_transaction_relay_auth_succeeds() {
    let (env, client, _admin, relay, contract_id) = setup_strict();
    let payload = valid_payload(&env);
    let tx_id = register_via_relay(&env, &client, &contract_id, &relay, &payload);

    client
        .mock_auths(&[MockAuth {
            address: &relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "start_processing",
                args: (tx_id.clone(), relay.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .start_processing(&tx_id, &relay);

    let hash = String::from_str(&env, "hash-success");
    client
        .mock_auths(&[MockAuth {
            address: &relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "complete_transaction",
                args: (tx_id.clone(), hash.clone(), relay.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .complete_transaction(&tx_id, &hash, &relay);

    assert_eq!(client.get_status(&tx_id), TransactionStatus::Completed);
}

// ═══════════════════════════════════════════════════════════════════════════════
// §5 — fail_transaction(tx_id, reason, caller)
// THREAT_MODEL.md §4.3
// ═══════════════════════════════════════════════════════════════════════════════

/// [`THREAT_MODEL` §4.3] Bystander cannot fail a transaction.
#[test]
fn auth_fail_transaction_bystander_rejected() {
    let (env, client, _admin, _relay) = setup();
    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);

    let bystander = Address::generate(&env);
    let reason = String::from_str(&env, "timeout");
    let result = client.try_fail_transaction(&tx_id, &reason, &bystander);
    assert_eq!(result, Err(Ok(ContractError::Unauthorised)));
}

/// [`THREAT_MODEL` §4.3] `caller` = relay, auth = admin — must fail (same mismatched
/// auth pattern as `start_processing` tests — validates no handler-specific gap).
#[test]
fn auth_fail_transaction_caller_relay_admin_auth_rejected() {
    let (env, client, admin, relay, contract_id) = setup_strict();
    let payload = valid_payload(&env);
    let tx_id = register_via_relay(&env, &client, &contract_id, &relay, &payload);

    let reason = String::from_str(&env, "timeout");
    let result = client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "fail_transaction",
                args: (tx_id.clone(), reason.clone(), relay.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_fail_transaction(&tx_id, &reason, &relay);

    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.3] Relay caller with relay auth on `fail_transaction` succeeds.
#[test]
fn auth_fail_transaction_relay_auth_succeeds() {
    let (env, client, _admin, relay, contract_id) = setup_strict();
    let payload = valid_payload(&env);
    let tx_id = register_via_relay(&env, &client, &contract_id, &relay, &payload);

    let reason = String::from_str(&env, "timeout");
    client
        .mock_auths(&[MockAuth {
            address: &relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "fail_transaction",
                args: (tx_id.clone(), reason.clone(), relay.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .fail_transaction(&tx_id, &reason, &relay);

    assert_eq!(client.get_status(&tx_id), TransactionStatus::Failed);
}

// ═══════════════════════════════════════════════════════════════════════════════
// §6 — propose_admin(new_admin)
// THREAT_MODEL.md §4.4
// ═══════════════════════════════════════════════════════════════════════════════

/// [`THREAT_MODEL` §4.4] Relay signer cannot propose an admin transfer.
/// Cross-role confusion: relay is in the system but not in the admin role.
#[test]
fn auth_propose_admin_relay_role_rejected() {
    let (env, client, _admin, relay, contract_id) = setup_strict();
    let new_admin = Address::generate(&env);

    let result = client
        .mock_auths(&[MockAuth {
            address: &relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "propose_admin",
                args: (new_admin.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_propose_admin(&new_admin);

    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.4] A completely unknown attacker cannot propose an admin transfer.
#[test]
fn auth_propose_admin_attacker_rejected() {
    let (env, client, _admin, _relay, contract_id) = setup_strict();
    let attacker = Address::generate(&env);
    let new_admin = Address::generate(&env);

    let result = client
        .mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "propose_admin",
                args: (new_admin.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_propose_admin(&new_admin);

    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.4 / F-02] Nominating the contract address itself is rejected.
#[test]
fn auth_propose_admin_contract_self_nomination_rejected() {
    let (_env, client, _admin, _relay) = setup();
    let result = client.try_propose_admin(&client.address);
    assert_eq!(result, Err(Ok(ContractError::InvalidAdminNominee)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// §7 — accept_admin(caller)
// THREAT_MODEL.md §4.4
// ═══════════════════════════════════════════════════════════════════════════════

/// [`THREAT_MODEL` §4.4] A bystander cannot accept a pending admin transfer even if
/// they forge a valid-looking `caller` argument.
#[test]
fn auth_accept_admin_bystander_as_caller_rejected() {
    let (env, client, _admin, _relay) = setup();
    let new_admin = Address::generate(&env);
    let bystander = Address::generate(&env);

    client.propose_admin(&new_admin);

    let result = client.try_accept_admin(&bystander);
    assert_eq!(result, Err(Ok(ContractError::Unauthorised)));
}

/// [`THREAT_MODEL` §4.4] The current admin cannot unilaterally finalise a transfer
/// by passing itself as `caller` to `accept_admin` — must be rejected.
/// This covers the "single-step admin transfer" finding F-03.
#[test]
fn auth_accept_admin_old_admin_cannot_self_finalize() {
    let (env, client, admin, _relay, contract_id) = setup_strict();
    let new_admin = Address::generate(&env);

    // Admin proposes itself as new admin (contrived but valid nominee).
    client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "propose_admin",
                args: (new_admin.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .propose_admin(&new_admin);

    // Old admin tries to accept on behalf of new_admin.
    let result = client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "accept_admin",
                args: (admin.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_accept_admin(&admin);

    // `admin` is not the pending admin, so Unauthorised.
    assert_eq!(result, Err(Ok(ContractError::Unauthorised)));
    // Admin is still the original admin.
    assert_eq!(client.admin(), admin);
}

/// [`THREAT_MODEL` §4.4] The relay signer cannot accept a pending admin transfer
/// even if there happens to be one in progress.
#[test]
fn auth_accept_admin_relay_cannot_accept() {
    // Use a fresh setup with mock_all_auths so we can easily propose, then
    // switch to scoped auth for the accept attempt.
    let (env, client, _admin, relay) = setup();
    let new_admin = Address::generate(&env);

    // Propose in mock_all_auths context.
    client.propose_admin(&new_admin);

    // Relay tries to accept as itself — must be rejected.
    let result = client.try_accept_admin(&relay);
    assert_eq!(result, Err(Ok(ContractError::Unauthorised)));
}

/// [`THREAT_MODEL` §4.4] When no transfer is pending, `accept_admin` must return
/// `NoPendingAdminTransfer` for any caller.
#[test]
fn auth_accept_admin_no_pending_transfer_rejected() {
    let (_env, client, admin, _relay) = setup();
    let result = client.try_accept_admin(&admin);
    assert_eq!(result, Err(Ok(ContractError::NoPendingAdminTransfer)));
}

/// [`THREAT_MODEL` §4.4] A superseded (overwritten) pending nominee can no longer
/// accept the transfer.
#[test]
fn auth_accept_admin_superseded_nominee_rejected() {
    let (env, client, _admin, _relay) = setup();
    let first_nominee = Address::generate(&env);
    let second_nominee = Address::generate(&env);

    client.propose_admin(&first_nominee);
    client.propose_admin(&second_nominee); // overwrites first

    let result = client.try_accept_admin(&first_nominee);
    assert_eq!(result, Err(Ok(ContractError::Unauthorised)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// §8 — set_relay_signer(new_signer)
// THREAT_MODEL.md §4.4
// ═══════════════════════════════════════════════════════════════════════════════

/// [`THREAT_MODEL` §4.4] The relay signer cannot rotate itself — admin-only.
#[test]
fn auth_set_relay_signer_relay_cannot_rotate_itself() {
    let (env, client, _admin, relay, contract_id) = setup_strict();
    let new_relay = Address::generate(&env);

    let result = client
        .mock_auths(&[MockAuth {
            address: &relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "set_relay_signer",
                args: (new_relay.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_set_relay_signer(&new_relay);

    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.4] An unknown attacker cannot rotate the relay signer.
#[test]
fn auth_set_relay_signer_attacker_rejected() {
    let (env, client, _admin, _relay, contract_id) = setup_strict();
    let attacker = Address::generate(&env);
    let new_relay = Address::generate(&env);

    let result = client
        .mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "set_relay_signer",
                args: (new_relay.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_set_relay_signer(&new_relay);

    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.4] After relay rotation, the OLD relay can no longer call
/// privileged operations (e.g. `start_processing`). This is the hardest "forged
/// signer" case — an old key that *was* valid but no longer is.
#[test]
fn auth_set_relay_signer_old_relay_locked_out_after_rotation() {
    let (env, client, admin, relay, contract_id) = setup_strict();
    let payload = valid_payload(&env);
    let tx_id = register_via_relay(&env, &client, &contract_id, &relay, &payload);

    // Rotate relay.
    let new_relay = Address::generate(&env);
    client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "set_relay_signer",
                args: (new_relay.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .set_relay_signer(&new_relay);

    // Old relay tries to start_processing — membership check returns Unauthorised.
    let result = client
        .mock_auths(&[MockAuth {
            address: &relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "start_processing",
                args: (tx_id.clone(), relay.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_start_processing(&tx_id, &relay);

    assert_eq!(result, Err(Ok(ContractError::Unauthorised)));
}

/// [`THREAT_MODEL` §4.4] After relay rotation, the NEW relay can perform
/// privileged operations — confirms rotation is effective.
#[test]
fn auth_set_relay_signer_new_relay_active_after_rotation() {
    let (env, client, admin, relay, contract_id) = setup_strict();
    let payload = valid_payload(&env);
    let tx_id = register_via_relay(&env, &client, &contract_id, &relay, &payload);

    let new_relay = Address::generate(&env);
    client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "set_relay_signer",
                args: (new_relay.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .set_relay_signer(&new_relay);

    client
        .mock_auths(&[MockAuth {
            address: &new_relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "start_processing",
                args: (tx_id.clone(), new_relay.clone()).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .start_processing(&tx_id, &new_relay);

    assert_eq!(client.get_status(&tx_id), TransactionStatus::Processing);
}

// ═══════════════════════════════════════════════════════════════════════════════
// §9 — upgrade(new_wasm_hash, expected_schema_version)
// THREAT_MODEL.md §4.5
// ═══════════════════════════════════════════════════════════════════════════════

/// [`THREAT_MODEL` §4.5] Relay signer cannot upgrade the contract — admin-only.
#[test]
fn auth_upgrade_relay_role_rejected() {
    let (env, client, _admin, relay, contract_id) = setup_strict();
    let dummy_hash = BytesN::from_array(&env, &[0u8; 32]);

    let result = client
        .mock_auths(&[MockAuth {
            address: &relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "upgrade",
                args: (dummy_hash.clone(), 1u32).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_upgrade(&dummy_hash, &1);

    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.5] An unrelated attacker cannot trigger an upgrade.
#[test]
fn auth_upgrade_attacker_rejected() {
    let (env, client, _admin, _relay, contract_id) = setup_strict();
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

    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.5 / F-04] Admin with correct schema version still rejected
/// when WASM hash does not exist in ledger — auth passes but deployer-side check
/// fails. We verify the schema check fires first (wrong version) before WASM lookup.
#[test]
fn auth_upgrade_schema_version_mismatch_rejected_before_wasm_lookup() {
    let (env, client, _admin, _relay) = setup();
    let dummy_hash = BytesN::from_array(&env, &[0u8; 32]);

    // Schema version on-chain is 1; passing 999 must be rejected.
    let result = client.try_upgrade(&dummy_hash, &999);
    assert_eq!(result, Err(Ok(ContractError::SchemaVersionMismatch)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// §10 — pause() / unpause()
// THREAT_MODEL.md §4.6
// ═══════════════════════════════════════════════════════════════════════════════

/// [`THREAT_MODEL` §4.6] The relay signer cannot pause the contract — admin-only.
#[test]
fn auth_pause_relay_role_rejected() {
    let (env, client, _admin, relay, contract_id) = setup_strict();

    let result = client
        .mock_auths(&[MockAuth {
            address: &relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "pause",
                args: ().into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_pause();

    assert!(result.is_err());
    assert!(!client.is_paused());
}

/// [`THREAT_MODEL` §4.6] An unknown attacker cannot pause the contract.
#[test]
fn auth_pause_attacker_rejected() {
    let (env, client, _admin, _relay, contract_id) = setup_strict();
    let attacker = Address::generate(&env);

    let result = client
        .mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "pause",
                args: ().into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_pause();

    assert!(result.is_err());
}

/// [`THREAT_MODEL` §4.6] The relay signer cannot unpause the contract.
#[test]
fn auth_unpause_relay_role_rejected() {
    let (env, client, _admin, relay, contract_id) = setup_strict();
    // Pause using mock_all_auths context.
    client.env.mock_all_auths();
    client.pause();

    let result = client
        .mock_auths(&[MockAuth {
            address: &relay,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "unpause",
                args: ().into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_unpause();

    assert!(result.is_err());
    assert!(client.is_paused());
}

// ═══════════════════════════════════════════════════════════════════════════════
// §11 — Cross-role confusion (systematic)
// THREAT_MODEL.md §2 actor model / §3 trust boundaries
// ═══════════════════════════════════════════════════════════════════════════════

/// [`THREAT_MODEL` §2 / §3] A valid relay signer must not be able to call any
/// admin-only entry point. Runs every admin-only function with relay auth.
#[test]
fn cross_role_relay_cannot_call_any_admin_only_function() {
    let (env, client, _admin, relay, contract_id) = setup_strict();
    let some_addr = Address::generate(&env);
    let dummy_hash = BytesN::from_array(&env, &[0u8; 32]);

    macro_rules! assert_relay_cannot {
        ($fn_name:expr, $args:expr, $($call:tt)+) => {{
            let result = client
                .mock_auths(&[MockAuth {
                    address: &relay,
                    invoke: &MockAuthInvoke {
                        contract: &contract_id,
                        fn_name: $fn_name,
                        args: $args,
                        sub_invokes: &[],
                    },
                }])
                .$($call)+;
            assert!(result.is_err(), "relay must not succeed on {}", $fn_name);
        }};
    }

    assert_relay_cannot!(
        "propose_admin",
        (some_addr.clone(),).into_val(&env),
        try_propose_admin(&some_addr)
    );
    assert_relay_cannot!(
        "set_relay_signer",
        (some_addr.clone(),).into_val(&env),
        try_set_relay_signer(&some_addr)
    );
    assert_relay_cannot!(
        "upgrade",
        (dummy_hash.clone(), 1u32).into_val(&env),
        try_upgrade(&dummy_hash, &1)
    );
    assert_relay_cannot!("pause", ().into_val(&env), try_pause());
    assert_relay_cannot!("unpause", ().into_val(&env), try_unpause());
}

/// [`THREAT_MODEL` §2 / §3] After an admin transfer, the old admin loses all
/// admin-only capabilities. Tests `propose_admin` specifically.
#[test]
fn cross_role_old_admin_loses_privileges_after_transfer() {
    let (env, client, admin, _relay, contract_id) = setup_strict();
    let new_admin = Address::generate(&env);
    let nominee = Address::generate(&env);

    // Transfer admin to new_admin.
    client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "propose_admin",
                args: (new_admin.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .propose_admin(&new_admin);

    client
        .mock_auths(&[MockAuth {
            address: &new_admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "accept_admin",
                args: (new_admin.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .accept_admin(&new_admin);

    assert_eq!(client.admin(), new_admin);

    // Old admin can no longer propose.
    let result = client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &MockAuthInvoke {
                contract: &contract_id,
                fn_name: "propose_admin",
                args: (nominee.clone(),).into_val(&env),
                sub_invokes: &[],
            },
        }])
        .try_propose_admin(&nominee);

    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// §12 — Replay / duplicate attempts
// THREAT_MODEL.md §4.2 (idempotency / F-07)
// ═══════════════════════════════════════════════════════════════════════════════

/// [`THREAT_MODEL` §4.2] Replaying the exact same payload twice must be deduplicated;
/// the second call must not emit a registration event and must return the same `tx_id`.
#[test]
fn replay_same_idempotency_key_deduped_silently() {
    let (env, client, _admin, _relay) = setup();
    let payload = valid_payload(&env);

    let id1 = client.register_callback(&payload);
    let id2 = client.register_callback(&payload);

    assert_eq!(id1, id2);
    // No second event should have been emitted.
    let events = client.env.events().all();
    assert_eq!(
        events.len(),
        0,
        "idempotent replay must not emit a reg event"
    );
}

/// [`THREAT_MODEL` §4.2 / F-07] After the idempotency TTL, a replay with a *fresh*
/// idempotency key but the same `transaction_id` must still be rejected via the
/// persistent `transaction_id` guard.
#[test]
fn replay_fresh_idem_key_same_tx_id_rejected_after_ttl() {
    use crate::types::StorageKey;

    let (env, client, _admin, _relay) = setup();
    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);

    // Drive to Completed so the record exists in a terminal state.
    let relay = client.relay_signer();
    client.start_processing(&tx_id, &relay);
    client.complete_transaction(&tx_id, &String::from_str(&env, "hash-replay"), &relay);

    // Extend persistent entries so the contract doesn't archive before we test.
    env.as_contract(&client.address, || {
        env.storage().instance().extend_ttl(200_000, 200_000);
        env.storage()
            .persistent()
            .extend_ttl(&StorageKey::Admin, 200_000, 200_000);
        env.storage()
            .persistent()
            .extend_ttl(&StorageKey::RelaySigner, 200_000, 200_000);
    });

    // Jump past the idempotency TTL (~18 000 ledgers).
    env.ledger().with_mut(|li| li.sequence_number += 18_001);

    let mut replay_payload = payload;
    replay_payload.idempotency_key = String::from_str(&env, "idem-replay-fresh");

    let result = client.try_register_callback(&replay_payload);
    assert_eq!(result, Err(Ok(ContractError::DuplicateRequest)));

    // Original record must be untouched.
    let tx = client.get_transaction(&tx_id);
    assert_eq!(tx.status, TransactionStatus::Completed);
    assert_eq!(tx.stellar_tx_hash, String::from_str(&env, "hash-replay"));
}

/// [`THREAT_MODEL` §4.3] Attempting to complete a transaction twice (once it has
/// already reached Completed) must be rejected — Completed is terminal.
#[test]
fn replay_complete_twice_rejected() {
    let (env, client, _admin, relay) = setup();
    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);
    client.start_processing(&tx_id, &relay);
    let hash = String::from_str(&env, "hash-dup");
    client.complete_transaction(&tx_id, &hash, &relay);

    // Second complete on an already-Completed tx.
    let result = client.try_complete_transaction(&tx_id, &hash, &relay);
    assert_eq!(result, Err(Ok(ContractError::InvalidStatusTransition)));
}

/// [`THREAT_MODEL` §4.3] Attempting to fail a Completed transaction must be
/// rejected — `complete_transaction` reaching `Completed` is terminal.
#[test]
fn replay_fail_after_complete_rejected() {
    let (env, client, _admin, relay) = setup();
    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);
    client.start_processing(&tx_id, &relay);
    client.complete_transaction(&tx_id, &String::from_str(&env, "hash-fin"), &relay);

    let result = client.try_fail_transaction(&tx_id, &String::from_str(&env, "too_late"), &relay);
    assert_eq!(result, Err(Ok(ContractError::InvalidStatusTransition)));
}

/// [`THREAT_MODEL` §4.3] Attempting to `start_processing` a Failed transaction must
/// be rejected — `Failed` is terminal.
#[test]
fn replay_start_processing_after_fail_rejected() {
    let (env, client, _admin, relay) = setup();
    let payload = valid_payload(&env);
    let tx_id = client.register_callback(&payload);
    client.fail_transaction(&tx_id, &String::from_str(&env, "reason"), &relay);

    let result = client.try_start_processing(&tx_id, &relay);
    assert_eq!(result, Err(Ok(ContractError::InvalidStatusTransition)));
}

// ═══════════════════════════════════════════════════════════════════════════════
// §13 — Uninitialised contract adversarial calls
// THREAT_MODEL.md §4.1 / I-08
// ═══════════════════════════════════════════════════════════════════════════════

/// [`THREAT_MODEL` §4.1 / I-08] All state-mutating calls on a freshly-deployed,
/// uninitialised contract must fail. Covers the implicit initialisation guard.
#[test]
fn auth_uninitialised_contract_all_mutating_calls_rejected() {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    env.mock_all_auths();

    let payload = valid_payload(&env);
    let some_addr = Address::generate(&env);
    let dummy_hash = BytesN::from_array(&env, &[0u8; 32]);
    let dummy_tx_id = String::from_str(&env, "no-such-tx");

    // register_callback reads relay signer first → NotInitialised.
    assert_eq!(
        client.try_register_callback(&payload),
        Err(Ok(ContractError::NotInitialised))
    );
    // status transitions also read admin/relay → NotInitialised.
    assert_eq!(
        client.try_start_processing(&dummy_tx_id, &some_addr),
        Err(Ok(ContractError::NotInitialised))
    );
    assert_eq!(
        client.try_complete_transaction(&dummy_tx_id, &String::from_str(&env, "h"), &some_addr),
        Err(Ok(ContractError::NotInitialised))
    );
    assert_eq!(
        client.try_fail_transaction(&dummy_tx_id, &String::from_str(&env, "r"), &some_addr),
        Err(Ok(ContractError::NotInitialised))
    );
    // Admin-only calls.
    assert_eq!(
        client.try_propose_admin(&some_addr),
        Err(Ok(ContractError::NotInitialised))
    );
    assert_eq!(
        client.try_set_relay_signer(&some_addr),
        Err(Ok(ContractError::NotInitialised))
    );
    assert_eq!(
        client.try_upgrade(&dummy_hash, &1),
        Err(Ok(ContractError::NotInitialised))
    );
    assert_eq!(client.try_pause(), Err(Ok(ContractError::NotInitialised)));
    assert_eq!(client.try_unpause(), Err(Ok(ContractError::NotInitialised)));
}
