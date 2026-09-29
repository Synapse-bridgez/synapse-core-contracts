//! # Formal State-Machine Model — Issue #127
//!
//! An executable model of the full [`TransactionStatus`] state machine.
//! Rather than testing one transition at a time alongside happy-path tests,
//! this module:
//!
//! 1. **Defines the allowed transition graph** as a pure Rust data structure.
//! 2. **Exhaustively enumerates** every `(from_state, entry_point)` pair and
//!    asserts that the contract's behaviour (accept / reject) exactly matches
//!    what the model declares.
//! 3. **Verifies structural invariants** of the model itself (no orphan states,
//!    all terminal states are truly reachable, etc.).
//!
//! ## State machine (current contract)
//!
//! ```text
//!   ┌─────────────────────────────────────────┐
//!   │              (created)                  │
//!   │                  ↓                      │
//!   │             Pending ──────────────────┐ │
//!   │                │                      │ │
//!   │        start_processing               │ │
//!   │                │                 fail_transaction
//!   │                ↓                      │ │
//!   │           Processing ─────────────────┤ │
//!   │                │                      │ │
//!   │    complete_transaction               ↓ │
//!   │                │                   Failed (terminal)
//!   │                ↓                        │
//!   │           Completed (terminal)          │
//!   └─────────────────────────────────────────┘
//! ```
//!
//! ## Model-checking approach
//!
//! Soroban's test environment is deterministic and synchronous, so full
//! enumeration is feasible: we iterate the Cartesian product
//! `TransactionStatus × EntryPoint` and drive each combination against a
//! live contract instance, comparing the outcome to the model's prediction.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, String};

use crate::types::{CallbackPayload, CallbackType, ContractError, TransactionStatus};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

// ─── Shared helpers ───────────────────────────────────────────────────────────

fn g_address(env: &Env) -> String {
    String::from_str(
        env,
        "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ",
    )
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

fn make_payload(env: &Env, tx_id: &str, idem: &str) -> CallbackPayload {
    let account = g_address(env);
    CallbackPayload {
        transaction_id: String::from_str(env, tx_id),
        stellar_account: account.clone(),
        amount: 1_000,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: account,
        idempotency_key: String::from_str(env, idem),
        anchor_transaction_id: String::from_str(env, "anc-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    }
}

// ─── Model definition ─────────────────────────────────────────────────────────

/// Every entry point that can change transaction status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryPoint {
    StartProcessing,
    CompleteTransaction,
    FailTransaction,
}

impl EntryPoint {
    fn all() -> &'static [EntryPoint] {
        &[
            EntryPoint::StartProcessing,
            EntryPoint::CompleteTransaction,
            EntryPoint::FailTransaction,
        ]
    }

    fn name(self) -> &'static str {
        match self {
            EntryPoint::StartProcessing => "start_processing",
            EntryPoint::CompleteTransaction => "complete_transaction",
            EntryPoint::FailTransaction => "fail_transaction",
        }
    }
}

/// All concrete `TransactionStatus` variants.
fn all_statuses() -> &'static [TransactionStatus] {
    &[
        TransactionStatus::Pending,
        TransactionStatus::Processing,
        TransactionStatus::Completed,
        TransactionStatus::Failed,
    ]
}

fn status_index(s: &TransactionStatus) -> usize {
    all_statuses()
        .iter()
        .position(|x| x == s)
        .expect("all_statuses() must be exhaustive")
}

/// The formal model: given `(from_status, entry_point)`, what is the expected
/// outcome?
///
/// Returns `Some(to_status)` for a valid transition, `None` for an invalid one
/// (which must produce `InvalidStatusTransition` from the contract).
fn model_transition(from: &TransactionStatus, entry: EntryPoint) -> Option<TransactionStatus> {
    match (from, entry) {
        // ── Valid transitions ──────────────────────────────────────────────
        (TransactionStatus::Pending, EntryPoint::StartProcessing) => {
            Some(TransactionStatus::Processing)
        }
        (TransactionStatus::Processing, EntryPoint::CompleteTransaction) => {
            Some(TransactionStatus::Completed)
        }
        (TransactionStatus::Pending, EntryPoint::FailTransaction) => {
            Some(TransactionStatus::Failed)
        }
        (TransactionStatus::Processing, EntryPoint::FailTransaction) => {
            Some(TransactionStatus::Failed)
        }
        // ── Invalid transitions — all other (from, entry) pairs ────────────
        _ => None,
    }
}

// ─── Model structural invariants ──────────────────────────────────────────────

/// Every status that has at least one outgoing edge in the model must be
/// reachable from `Pending` via some sequence of valid transitions.
/// We verify this by explicit reachability enumeration over the small 4-node graph.
#[test]
fn model_all_states_reachable_from_pending() {
    // Manual BFS over the 4-state graph — no std::collections needed.
    // visited[i] corresponds to all_statuses()[i].
    let statuses = all_statuses();
    let mut visited = [false; 4]; // indexed parallel to all_statuses()
    let mut queue = [0usize; 16]; // simple queue; 4 states max depth
    let mut head = 0usize;
    let mut tail = 0usize;

    // Start from Pending (index 0).
    visited[0] = true;
    queue[tail] = 0;
    tail += 1;

    while head < tail {
        let idx = queue[head];
        head += 1;
        let state = &statuses[idx];

        for &ep in EntryPoint::all() {
            if let Some(next) = model_transition(state, ep) {
                let next_idx = status_index(&next);
                if !visited[next_idx] {
                    visited[next_idx] = true;
                    queue[tail] = next_idx;
                    tail += 1;
                }
            }
        }
    }

    for (i, s) in statuses.iter().enumerate() {
        assert!(
            visited[i],
            "status {:?} is not reachable from Pending in the model",
            s
        );
    }
}

/// Terminal states (`Completed`, `Failed`) must have NO outgoing edges in the model.
#[test]
fn model_terminal_states_have_no_outgoing_transitions() {
    let terminals = [TransactionStatus::Completed, TransactionStatus::Failed];
    for terminal in &terminals {
        for &ep in EntryPoint::all() {
            assert!(
                model_transition(terminal, ep).is_none(),
                "terminal state {:?} must not allow {:?}",
                terminal,
                ep.name()
            );
        }
    }
}

/// Non-terminal states must have at least one valid outgoing transition.
#[test]
fn model_non_terminal_states_have_at_least_one_outgoing_transition() {
    let non_terminals = [TransactionStatus::Pending, TransactionStatus::Processing];
    for state in &non_terminals {
        let has_exit = EntryPoint::all()
            .iter()
            .any(|&ep| model_transition(state, ep).is_some());
        assert!(
            has_exit,
            "non-terminal state {:?} must have at least one valid outgoing transition",
            state
        );
    }
}

/// The model must have exactly 4 valid transitions (the ones listed in README.md).
#[test]
fn model_transition_count_matches_spec() {
    let count: usize = all_statuses()
        .iter()
        .flat_map(|s| EntryPoint::all().iter().map(move |&ep| (s, ep)))
        .filter(|(s, ep)| model_transition(s, *ep).is_some())
        .count();

    // Pending→Processing, Processing→Completed, Pending→Failed, Processing→Failed
    assert_eq!(
        count, 4,
        "expected exactly 4 valid transitions in the model"
    );
}

// ─── Exhaustive contract conformance check ────────────────────────────────────

/// Helper: put a fresh transaction into `target_status` by driving the minimal
/// transition sequence. Returns the `tx_id` used.
fn drive_to_status(
    client: &SynapseCoreContractClient,
    env: &Env,
    relay: &Address,
    target: &TransactionStatus,
    tx_id: &str,
    idem: &str,
) -> String {
    let payload = make_payload(env, tx_id, idem);
    let id = client.register_callback(&payload);

    match target {
        TransactionStatus::Pending => {
            // Already Pending after register_callback.
        }
        TransactionStatus::Processing => {
            client.start_processing(&id, relay);
        }
        TransactionStatus::Completed => {
            client.start_processing(&id, relay);
            client.complete_transaction(&id, &String::from_str(env, "hash-sm"), relay);
        }
        TransactionStatus::Failed => {
            client.fail_transaction(&id, &String::from_str(env, "reason-sm"), relay);
        }
    }

    id
}

// Static lookup table mapping (status_idx, ep_idx) to a unique tx_id and idem key.
// 4 statuses × 3 entry points = 12 pairs. IDs must fit within MAX_TX_ID_LEN (64).
const TX_IDS: [[&str; 3]; 4] = [
    ["tx-sm-p-sp", "tx-sm-p-ct", "tx-sm-p-ft"],    // Pending
    ["tx-sm-pr-sp", "tx-sm-pr-ct", "tx-sm-pr-ft"], // Processing
    ["tx-sm-c-sp", "tx-sm-c-ct", "tx-sm-c-ft"],    // Completed
    ["tx-sm-f-sp", "tx-sm-f-ct", "tx-sm-f-ft"],    // Failed
];

const IDEM_IDS: [[&str; 3]; 4] = [
    ["id-sm-p-sp", "id-sm-p-ct", "id-sm-p-ft"],
    ["id-sm-pr-sp", "id-sm-pr-ct", "id-sm-pr-ft"],
    ["id-sm-c-sp", "id-sm-c-ct", "id-sm-c-ft"],
    ["id-sm-f-sp", "id-sm-f-ct", "id-sm-f-ft"],
];

/// For every `(from_status, entry_point)` pair, drive a real contract instance
/// to `from_status` and invoke `entry_point`. Compare the outcome to the model.
///
/// This is the core conformance check: contract behaviour must exactly match
/// the formal model for all 12 pairs (4 states × 3 entry points).
#[test]
fn exhaustive_transition_conformance_with_model() {
    for (status_idx, from_status) in all_statuses().iter().enumerate() {
        for (ep_idx, &entry_point) in EntryPoint::all().iter().enumerate() {
            // Fresh contract per (state, entry_point) pair to avoid state bleed.
            let (env, client, _admin, relay) = setup();

            let tx_id_str = TX_IDS[status_idx][ep_idx];
            let idem_str = IDEM_IDS[status_idx][ep_idx];

            let id = drive_to_status(&client, &env, &relay, from_status, tx_id_str, idem_str);

            // Confirm we actually reached the desired state.
            assert_eq!(
                client.get_status(&id),
                *from_status,
                "pre-condition: failed to drive tx to {:?} (status_idx={}, ep_idx={})",
                from_status,
                status_idx,
                ep_idx
            );

            let predicted = model_transition(from_status, entry_point);
            let hash = String::from_str(&env, "hash-exhaust");
            let reason = String::from_str(&env, "reason-exhaust");

            match entry_point {
                EntryPoint::StartProcessing => {
                    let result = client.try_start_processing(&id, &relay);
                    match predicted {
                        Some(expected_next) => {
                            assert!(
                                result.is_ok(),
                                "Model says {:?} --start_processing--> {:?} is valid, \
                                 but contract returned Err (status_idx={}, ep_idx={})",
                                from_status,
                                expected_next,
                                status_idx,
                                ep_idx
                            );
                            assert_eq!(client.get_status(&id), expected_next);
                        }
                        None => {
                            assert_eq!(
                                result,
                                Err(Ok(ContractError::InvalidStatusTransition)),
                                "Model says {:?} --start_processing--> INVALID, \
                                 but contract did not return InvalidStatusTransition \
                                 (status_idx={}, ep_idx={})",
                                from_status,
                                status_idx,
                                ep_idx
                            );
                        }
                    }
                }

                EntryPoint::CompleteTransaction => {
                    let result = client.try_complete_transaction(&id, &hash, &relay);
                    match predicted {
                        Some(expected_next) => {
                            assert!(
                                result.is_ok(),
                                "Model says {:?} --complete_transaction--> {:?} is valid, \
                                 but contract returned Err (status_idx={}, ep_idx={})",
                                from_status,
                                expected_next,
                                status_idx,
                                ep_idx
                            );
                            assert_eq!(client.get_status(&id), expected_next);
                        }
                        None => {
                            assert_eq!(
                                result,
                                Err(Ok(ContractError::InvalidStatusTransition)),
                                "Model says {:?} --complete_transaction--> INVALID, \
                                 but contract did not return InvalidStatusTransition \
                                 (status_idx={}, ep_idx={})",
                                from_status,
                                status_idx,
                                ep_idx
                            );
                        }
                    }
                }

                EntryPoint::FailTransaction => {
                    let result = client.try_fail_transaction(&id, &reason, &relay);
                    match predicted {
                        Some(expected_next) => {
                            assert!(
                                result.is_ok(),
                                "Model says {:?} --fail_transaction--> {:?} is valid, \
                                 but contract returned Err (status_idx={}, ep_idx={})",
                                from_status,
                                expected_next,
                                status_idx,
                                ep_idx
                            );
                            assert_eq!(client.get_status(&id), expected_next);
                        }
                        None => {
                            assert_eq!(
                                result,
                                Err(Ok(ContractError::InvalidStatusTransition)),
                                "Model says {:?} --fail_transaction--> INVALID, \
                                 but contract did not return InvalidStatusTransition \
                                 (status_idx={}, ep_idx={})",
                                from_status,
                                status_idx,
                                ep_idx
                            );
                        }
                    }
                }
            }
        }
    }
}

// ─── State-field invariants — data consistency after each transition ──────────

/// After `register_callback` the transaction must start in Pending with empty
/// sentinel fields.
#[test]
fn sm_register_creates_pending_with_empty_sentinels() {
    let (env, client, _admin, _relay) = setup();
    let payload = make_payload(&env, "tx-sentinels", "idem-sentinels");
    let id = client.register_callback(&payload);
    let tx = client.get_transaction(&id);

    assert_eq!(tx.status, TransactionStatus::Pending);
    assert_eq!(tx.stellar_tx_hash, String::from_str(&env, ""));
    assert_eq!(tx.failure_reason, String::from_str(&env, ""));
    assert_eq!(tx.updated_at_ledger, tx.created_at_ledger);
}

/// After `complete_transaction` the hash is set and `failure_reason` stays empty.
#[test]
fn sm_complete_sets_hash_and_leaves_failure_reason_empty() {
    let (env, client, _admin, relay) = setup();
    let payload = make_payload(&env, "tx-complete-fields", "idem-complete-fields");
    let id = client.register_callback(&payload);
    client.start_processing(&id, &relay);

    let hash = String::from_str(&env, "stellar-hash-abc");
    client.complete_transaction(&id, &hash, &relay);

    let tx = client.get_transaction(&id);
    assert_eq!(tx.status, TransactionStatus::Completed);
    assert_eq!(tx.stellar_tx_hash, hash);
    assert_eq!(tx.failure_reason, String::from_str(&env, ""));
    assert!(tx.updated_at_ledger >= tx.created_at_ledger);
}

/// After `fail_transaction` the reason is set and `stellar_tx_hash` stays empty.
#[test]
fn sm_fail_sets_reason_and_leaves_hash_empty() {
    let (env, client, _admin, relay) = setup();
    let payload = make_payload(&env, "tx-fail-fields", "idem-fail-fields");
    let id = client.register_callback(&payload);

    let reason = String::from_str(&env, "horizon_timeout");
    client.fail_transaction(&id, &reason, &relay);

    let tx = client.get_transaction(&id);
    assert_eq!(tx.status, TransactionStatus::Failed);
    assert_eq!(tx.failure_reason, reason);
    assert_eq!(tx.stellar_tx_hash, String::from_str(&env, ""));
    assert!(tx.updated_at_ledger >= tx.created_at_ledger);
}

/// `updated_at_ledger` must be strictly non-decreasing across the full lifecycle.
#[test]
fn sm_updated_at_ledger_monotonically_non_decreasing() {
    let (env, client, _admin, relay) = setup();
    let payload = make_payload(&env, "tx-ledger-mono", "idem-ledger-mono");
    let id = client.register_callback(&payload);

    let t0 = client.get_transaction(&id).updated_at_ledger;

    client.start_processing(&id, &relay);
    let t1 = client.get_transaction(&id).updated_at_ledger;

    client.complete_transaction(&id, &String::from_str(&env, "h"), &relay);
    let t2 = client.get_transaction(&id).updated_at_ledger;

    assert!(
        t1 >= t0,
        "updated_at_ledger went backwards: t0={t0} t1={t1}"
    );
    assert!(
        t2 >= t1,
        "updated_at_ledger went backwards: t1={t1} t2={t2}"
    );
}

/// `created_at_ledger` must never change once set.
#[test]
fn sm_created_at_ledger_immutable_through_lifecycle() {
    let (env, client, _admin, relay) = setup();
    let payload = make_payload(&env, "tx-created-imm", "idem-created-imm");
    let id = client.register_callback(&payload);
    let created = client.get_transaction(&id).created_at_ledger;

    client.start_processing(&id, &relay);
    assert_eq!(client.get_transaction(&id).created_at_ledger, created);

    client.complete_transaction(&id, &String::from_str(&env, "h"), &relay);
    assert_eq!(client.get_transaction(&id).created_at_ledger, created);
}

// ─── Skip-state path enumeration ─────────────────────────────────────────────

/// `Pending → Completed` (skipping Processing) is forbidden.
#[test]
fn sm_skip_pending_to_completed_rejected() {
    let (env, client, _admin, relay) = setup();
    let payload = make_payload(&env, "tx-skip-pend-comp", "idem-skip-pend-comp");
    let id = client.register_callback(&payload);

    let result = client.try_complete_transaction(&id, &String::from_str(&env, "hash"), &relay);
    assert_eq!(result, Err(Ok(ContractError::InvalidStatusTransition)));
    assert_eq!(client.get_status(&id), TransactionStatus::Pending);
}

/// `Completed → Processing` (re-entering a terminal state) is forbidden.
#[test]
fn sm_completed_to_processing_rejected() {
    let (env, client, _admin, relay) = setup();
    let payload = make_payload(&env, "tx-comp-proc", "idem-comp-proc");
    let id = client.register_callback(&payload);
    client.start_processing(&id, &relay);
    client.complete_transaction(&id, &String::from_str(&env, "h"), &relay);

    let result = client.try_start_processing(&id, &relay);
    assert_eq!(result, Err(Ok(ContractError::InvalidStatusTransition)));
    assert_eq!(client.get_status(&id), TransactionStatus::Completed);
}

/// `Failed → Processing` (re-entering a terminal state) is forbidden.
#[test]
fn sm_failed_to_processing_rejected() {
    let (env, client, _admin, relay) = setup();
    let payload = make_payload(&env, "tx-fail-proc", "idem-fail-proc");
    let id = client.register_callback(&payload);
    client.fail_transaction(&id, &String::from_str(&env, "r"), &relay);

    let result = client.try_start_processing(&id, &relay);
    assert_eq!(result, Err(Ok(ContractError::InvalidStatusTransition)));
}

/// `Failed → Completed` is forbidden.
#[test]
fn sm_failed_to_completed_rejected() {
    let (env, client, _admin, relay) = setup();
    let payload = make_payload(&env, "tx-fail-comp", "idem-fail-comp");
    let id = client.register_callback(&payload);
    client.fail_transaction(&id, &String::from_str(&env, "r"), &relay);

    let result = client.try_complete_transaction(&id, &String::from_str(&env, "h"), &relay);
    assert_eq!(result, Err(Ok(ContractError::InvalidStatusTransition)));
}

/// `Completed → Failed` is forbidden.
#[test]
fn sm_completed_to_failed_rejected() {
    let (env, client, _admin, relay) = setup();
    let payload = make_payload(&env, "tx-comp-fail", "idem-comp-fail");
    let id = client.register_callback(&payload);
    client.start_processing(&id, &relay);
    client.complete_transaction(&id, &String::from_str(&env, "h"), &relay);

    let result = client.try_fail_transaction(&id, &String::from_str(&env, "r"), &relay);
    assert_eq!(result, Err(Ok(ContractError::InvalidStatusTransition)));
}

/// `Processing → Processing` (self-loop) is forbidden.
#[test]
fn sm_processing_to_processing_self_loop_rejected() {
    let (env, client, _admin, relay) = setup();
    let payload = make_payload(&env, "tx-proc-loop", "idem-proc-loop");
    let id = client.register_callback(&payload);
    client.start_processing(&id, &relay);

    let result = client.try_start_processing(&id, &relay);
    assert_eq!(result, Err(Ok(ContractError::InvalidStatusTransition)));
}
