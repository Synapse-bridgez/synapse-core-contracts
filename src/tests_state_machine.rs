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
//!                  (created)
//!                      ↓
//!   ┌──────────────► Pending ─────────────┬──────────────┐
//!   │                   │                 │              │
//!   │           start_processing   fail_transaction  cancel_transaction
//!   │                   ↓                 │              │
//!   │              Processing ──fail──────┤              │
//!   │                   │      └──cancel──┼──────────────┤
//!   │       complete_transaction          ↓              ↓
//!   │                   ↓               Failed       Cancelled (terminal)
//!   │           Completed (terminal)      │
//!   └──────── retry_transaction ──────────┘  (bounded by MAX_RETRIES)
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
    CancelTransaction,
    RetryTransaction,
}

impl EntryPoint {
    fn all() -> &'static [EntryPoint] {
        &[
            EntryPoint::StartProcessing,
            EntryPoint::CompleteTransaction,
            EntryPoint::FailTransaction,
            EntryPoint::CancelTransaction,
            EntryPoint::RetryTransaction,
        ]
    }

    fn name(self) -> &'static str {
        match self {
            EntryPoint::StartProcessing => "start_processing",
            EntryPoint::CompleteTransaction => "complete_transaction",
            EntryPoint::FailTransaction => "fail_transaction",
            EntryPoint::CancelTransaction => "cancel_transaction",
            EntryPoint::RetryTransaction => "retry_transaction",
        }
    }

    /// Invoke this entry point against `id` as `relay`.
    fn invoke(
        self,
        client: &SynapseCoreContractClient,
        env: &Env,
        relay: &Address,
        id: &String,
    ) -> Result<(), ContractError> {
        let hash = String::from_str(env, "hash-exhaust");
        let reason = String::from_str(env, "reason-exhaust");
        let result = match self {
            EntryPoint::StartProcessing => client.try_start_processing(id, relay),
            EntryPoint::CompleteTransaction => client.try_complete_transaction(id, &hash, relay),
            EntryPoint::FailTransaction => client.try_fail_transaction(id, &reason, relay),
            EntryPoint::CancelTransaction => client.try_cancel_transaction(id, &reason, relay),
            EntryPoint::RetryTransaction => client.try_retry_transaction(id, relay),
        };
        match result {
            Ok(Ok(())) => Ok(()),
            Err(Ok(e)) => Err(e),
            other => panic!("{}: unexpected host-level result {:?}", self.name(), other),
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
        TransactionStatus::Cancelled,
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
/// (which must produce [`model_rejection`] from the contract). `retry` from
/// `Failed` is modelled with retries remaining; the `MAX_RETRIES` bound is
/// covered by `tests::test_retry_transaction_cycle_and_limit`.
fn model_transition(from: &TransactionStatus, entry: EntryPoint) -> Option<TransactionStatus> {
    use TransactionStatus::*;
    match (from, entry) {
        // ── Valid transitions ──────────────────────────────────────────────
        (Pending, EntryPoint::StartProcessing) => Some(Processing),
        (Processing, EntryPoint::CompleteTransaction) => Some(Completed),
        (Pending | Processing, EntryPoint::FailTransaction) => Some(Failed),
        (Pending | Processing, EntryPoint::CancelTransaction) => Some(Cancelled),
        (Failed, EntryPoint::RetryTransaction) => Some(Pending),
        // ── Invalid transitions — all other (from, entry) pairs ────────────
        _ => None,
    }
}

/// The error the contract must return for an invalid `(from, entry)` pair.
fn model_rejection(from: &TransactionStatus, entry: EntryPoint) -> ContractError {
    match (from, entry) {
        (TransactionStatus::Cancelled, EntryPoint::CancelTransaction) => {
            ContractError::AlreadyCancelled
        }
        (_, EntryPoint::CancelTransaction) => ContractError::CannotCancel,
        _ => ContractError::InvalidStatusTransition,
    }
}

// ─── Model structural invariants ──────────────────────────────────────────────

/// Every status that has at least one outgoing edge in the model must be
/// reachable from `Pending` via some sequence of valid transitions.
/// We verify this by explicit reachability enumeration over the small 5-node graph.
#[test]
fn model_all_states_reachable_from_pending() {
    // Manual BFS over the 5-state graph — no std::collections needed.
    // visited[i] corresponds to all_statuses()[i].
    let statuses = all_statuses();
    let mut visited = [false; 5]; // indexed parallel to all_statuses()
    let mut queue = [0usize; 16]; // simple queue; each state is enqueued once
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

/// Terminal states (`Completed`, `Cancelled`) must have NO outgoing edges in
/// the model. `Failed` is not terminal: `retry_transaction` re-opens it.
#[test]
fn model_terminal_states_have_no_outgoing_transitions() {
    let terminals = [TransactionStatus::Completed, TransactionStatus::Cancelled];
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
    let non_terminals = [
        TransactionStatus::Pending,
        TransactionStatus::Processing,
        TransactionStatus::Failed,
    ];
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

/// The model must have exactly 7 valid transitions (the ones in the diagram).
#[test]
fn model_transition_count_matches_spec() {
    let count: usize = all_statuses()
        .iter()
        .flat_map(|s| EntryPoint::all().iter().map(move |&ep| (s, ep)))
        .filter(|(s, ep)| model_transition(s, *ep).is_some())
        .count();

    // Pending→Processing, Processing→Completed, {Pending,Processing}→Failed,
    // {Pending,Processing}→Cancelled, Failed→Pending
    assert_eq!(
        count, 7,
        "expected exactly 7 valid transitions in the model"
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
        TransactionStatus::Cancelled => {
            client.cancel_transaction(&id, &String::from_str(env, "reason-sm"), relay);
        }
    }

    id
}

/// For every `(from_status, entry_point)` pair, drive a real contract instance
/// to `from_status` and invoke `entry_point`. Compare the outcome to the model.
///
/// This is the core conformance check: contract behaviour must exactly match
/// the formal model for all 25 pairs (5 states × 5 entry points), including
/// the specific error code of every rejection.
#[test]
fn exhaustive_transition_conformance_with_model() {
    for from_status in all_statuses() {
        for &entry_point in EntryPoint::all() {
            // Fresh contract per (state, entry_point) pair to avoid state bleed.
            let (env, client, _admin, relay) = setup();
            let id = drive_to_status(&client, &env, &relay, from_status, "tx-sm", "idem-sm");

            // Confirm we actually reached the desired state.
            assert_eq!(
                client.get_status(&id),
                *from_status,
                "pre-condition: failed to drive tx to {:?}",
                from_status
            );

            let result = entry_point.invoke(&client, &env, &relay, &id);
            match model_transition(from_status, entry_point) {
                Some(expected_next) => {
                    assert_eq!(
                        result,
                        Ok(()),
                        "Model says {:?} --{}--> {:?} is valid",
                        from_status,
                        entry_point.name(),
                        expected_next
                    );
                    assert_eq!(client.get_status(&id), expected_next);
                }
                None => {
                    assert_eq!(
                        result,
                        Err(model_rejection(from_status, entry_point)),
                        "Model says {:?} --{}--> INVALID",
                        from_status,
                        entry_point.name()
                    );
                    assert_eq!(
                        client.get_status(&id),
                        *from_status,
                        "a rejected {} must leave the status unchanged",
                        entry_point.name()
                    );
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
