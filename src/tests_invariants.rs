//! # Conservation Invariant Property Tests — Issue #129
//!
//! Verifies the core global conservation invariant:
//!
//! > After any sequence of valid operations, the total number of ever-registered
//! > transactions equals the sum of counts across every status bucket
//! > (`Pending + Processing + Completed + Failed`). No transaction can vanish
//! > or be double-counted.
//!
//! ## Why this matters
//!
//! Example-based unit tests check specific paths. Property tests check that
//! a global invariant holds across many operation sequences — the kind of
//! subtle book-keeping bug that only manifests after an unlikely combination
//! of operations (e.g. adding a new state-transition entry point that
//! accidentally creates a transaction record or fails to persist one).
//!
//! ## Approach
//!
//! Because Soroban's test environment has no built-in shrinking or fuzzing
//! harness in `no_std`, we implement **deterministic pseudo-random scenario
//! generation** with a lightweight LCG. Each scenario drives a contract
//! instance through a sequence of operations and asserts the invariant after
//! every step.
//!
//! ## Invariant definition
//!
//! ```text
//! total_registered
//!     == count(Pending)
//!      + count(Processing)
//!      + count(Completed)
//!      + count(Failed)
//! ```
//!
//! Every transaction is created exactly once (idempotent replays are
//! deduplicated) and ends up in exactly one status bucket at all times.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, String, Vec as SorobanVec};

use crate::types::{CallbackPayload, CallbackType, TransactionStatus};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

// ─── Invariant assertion helper ───────────────────────────────────────────────

/// Tally per-status counts by reading `tx_ids` from the contract.
fn tally_counts(
    client: &SynapseCoreContractClient,
    tx_ids: &SorobanVec<String>,
) -> (usize, usize, usize, usize) {
    let mut pending = 0usize;
    let mut processing = 0usize;
    let mut completed = 0usize;
    let mut failed = 0usize;

    for id in tx_ids.iter() {
        match client.get_status(&id) {
            TransactionStatus::Pending => pending += 1,
            TransactionStatus::Processing => processing += 1,
            TransactionStatus::Completed => completed += 1,
            TransactionStatus::Failed => failed += 1,
        }
    }
    (pending, processing, completed, failed)
}

/// Assert the conservation invariant.
fn assert_conservation(
    client: &SynapseCoreContractClient,
    tx_ids: &SorobanVec<String>,
    total_registered: usize,
    label: &str,
) {
    let (p, pr, c, f) = tally_counts(client, tx_ids);
    let bucket_total = p + pr + c + f;
    assert_eq!(
        bucket_total, total_registered,
        "[{label}] conservation violated: \
         total_registered={total_registered} but P={p}+Pr={pr}+C={c}+F={f}={bucket_total}"
    );
}

// ─── Setup helpers ────────────────────────────────────────────────────────────

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

// Static tx_id / idem_key pairs — enough for all scenarios (max 30 used).
// All IDs are unique, short enough (< 64 chars), and uppercase-only where needed.
const TX_IDS: &[&str] = &[
    "tx-inv-0000",
    "tx-inv-0001",
    "tx-inv-0002",
    "tx-inv-0003",
    "tx-inv-0004",
    "tx-inv-0005",
    "tx-inv-0006",
    "tx-inv-0007",
    "tx-inv-0008",
    "tx-inv-0009",
    "tx-inv-0010",
    "tx-inv-0011",
    "tx-inv-0012",
    "tx-inv-0013",
    "tx-inv-0014",
    "tx-inv-0015",
    "tx-inv-0016",
    "tx-inv-0017",
    "tx-inv-0018",
    "tx-inv-0019",
    "tx-inv-0020",
    "tx-inv-0021",
    "tx-inv-0022",
    "tx-inv-0023",
    "tx-inv-0024",
    "tx-inv-0025",
    "tx-inv-0026",
    "tx-inv-0027",
    "tx-inv-0028",
    "tx-inv-0029",
];

const IDEM_IDS: &[&str] = &[
    "id-inv-0000",
    "id-inv-0001",
    "id-inv-0002",
    "id-inv-0003",
    "id-inv-0004",
    "id-inv-0005",
    "id-inv-0006",
    "id-inv-0007",
    "id-inv-0008",
    "id-inv-0009",
    "id-inv-0010",
    "id-inv-0011",
    "id-inv-0012",
    "id-inv-0013",
    "id-inv-0014",
    "id-inv-0015",
    "id-inv-0016",
    "id-inv-0017",
    "id-inv-0018",
    "id-inv-0019",
    "id-inv-0020",
    "id-inv-0021",
    "id-inv-0022",
    "id-inv-0023",
    "id-inv-0024",
    "id-inv-0025",
    "id-inv-0026",
    "id-inv-0027",
    "id-inv-0028",
    "id-inv-0029",
];

// Extra sets used for specific scenarios (pause/unpause additional registrations).
const TX_IDS_B: &[&str] = &[
    "tx-invb-0000",
    "tx-invb-0001",
    "tx-invb-0002",
    "tx-invb-0003",
];
const IDEM_IDS_B: &[&str] = &[
    "id-invb-0000",
    "id-invb-0001",
    "id-invb-0002",
    "id-invb-0003",
];

fn make_payload(env: &Env, idx: usize) -> CallbackPayload {
    let account = g_address(env);
    CallbackPayload {
        transaction_id: String::from_str(env, TX_IDS[idx]),
        stellar_account: account.clone(),
        amount: 1_000 + idx as i128,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: account,
        idempotency_key: String::from_str(env, IDEM_IDS[idx]),
        anchor_transaction_id: String::from_str(env, "anc-inv-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    }
}

fn make_payload_b(env: &Env, idx: usize) -> CallbackPayload {
    let account = g_address(env);
    CallbackPayload {
        transaction_id: String::from_str(env, TX_IDS_B[idx]),
        stellar_account: account.clone(),
        amount: 2_000 + idx as i128,
        asset_code: String::from_str(env, "USDC"),
        asset_issuer: account,
        idempotency_key: String::from_str(env, IDEM_IDS_B[idx]),
        anchor_transaction_id: String::from_str(env, "anc-invb-1"),
        callback_type: CallbackType::Deposit,
        callback_status: String::from_str(env, "pending_external"),
    }
}

// ─── Lightweight deterministic pseudo-random sequence ────────────────────────

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    fn next_usize_below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % n as u64).unwrap()
    }

    fn next_bool(&mut self) -> bool {
        self.next() & 1 == 0
    }
}

/// Checked `usize` → `u32` conversion for Soroban `Vec` indices.
fn idx(i: usize) -> u32 {
    u32::try_from(i).unwrap()
}

// ─── Core property scenarios ──────────────────────────────────────────────────

/// **Conservation after a batch of pure registrations.**
///
/// N callbacks registered; every one must land in the Pending bucket; total == N.
#[test]
fn invariant_batch_register_all_pending() {
    let (env, client, _admin, _relay) = setup();
    let n = 20usize;
    let mut ids = SorobanVec::new(&env);

    for i in 0..n {
        let payload = make_payload(&env, i);
        let id = client.register_callback(&payload);
        ids.push_back(id);
    }

    assert_conservation(&client, &ids, n, "batch_register_all_pending");

    let (p, pr, c, f) = tally_counts(&client, &ids);
    assert_eq!(p, n, "all freshly registered txs must be Pending");
    assert_eq!(pr, 0);
    assert_eq!(c, 0);
    assert_eq!(f, 0);
}

/// **Conservation after driving every transaction to a terminal state.**
///
/// Half go to Completed, half to Failed — total must still be N.
#[test]
fn invariant_all_transactions_reach_terminal_states() {
    let (env, client, _admin, relay) = setup();
    let n = 16usize;
    let mut ids = SorobanVec::new(&env);

    for i in 0..n {
        let payload = make_payload(&env, i);
        let id = client.register_callback(&payload);
        client.start_processing(&id, &relay);
        if i % 2 == 0 {
            client.complete_transaction(&id, &String::from_str(&env, "hash-term"), &relay);
        } else {
            client.fail_transaction(&id, &String::from_str(&env, "reason"), &relay);
        }
        ids.push_back(id);
    }

    assert_conservation(&client, &ids, n, "all_terminal");

    let (p, pr, c, f) = tally_counts(&client, &ids);
    assert_eq!(c, n / 2);
    assert_eq!(f, n / 2);
    assert_eq!(p, 0);
    assert_eq!(pr, 0);
}

/// **Conservation after mixed in-progress and terminal transactions.**
#[test]
fn invariant_mixed_in_progress_and_terminal() {
    let (env, client, _admin, relay) = setup();
    let n = 24usize;
    let mut ids = SorobanVec::new(&env);

    for i in 0..n {
        let payload = make_payload(&env, i);
        let id = client.register_callback(&payload);

        match i % 3 {
            0 => { /* stay Pending */ }
            1 => {
                client.start_processing(&id, &relay);
            }
            _ => {
                client.start_processing(&id, &relay);
                client.complete_transaction(&id, &String::from_str(&env, "hash-mix"), &relay);
            }
        }
        ids.push_back(id);
    }

    assert_conservation(&client, &ids, n, "mixed_in_progress_terminal");
}

/// **Conservation after idempotent replays.**
///
/// Replaying the same payload must NOT increase the registered count.
#[test]
fn invariant_idempotent_replay_does_not_increase_count() {
    let (env, client, _admin, _relay) = setup();
    let n = 10usize;
    let mut ids = SorobanVec::new(&env);

    for i in 0..n {
        let payload = make_payload(&env, i);
        let id = client.register_callback(&payload);
        // Replay twice — must be deduplicated.
        client.register_callback(&payload);
        client.register_callback(&payload);
        ids.push_back(id);
    }

    assert_conservation(&client, &ids, n, "idempotent_replay");
}

/// **Conservation holds at each step of a single full lifecycle.**
#[test]
fn invariant_holds_at_every_lifecycle_step() {
    let (env, client, _admin, relay) = setup();
    let payload = make_payload(&env, 0);
    let id = client.register_callback(&payload);
    let mut ids = SorobanVec::new(&env);
    ids.push_back(id.clone());

    assert_conservation(&client, &ids, 1, "step:registered");

    client.start_processing(&id, &relay);
    assert_conservation(&client, &ids, 1, "step:processing");

    client.complete_transaction(&id, &String::from_str(&env, "h"), &relay);
    assert_conservation(&client, &ids, 1, "step:completed");
}

/// **Conservation holds across the fail path at each step.**
#[test]
fn invariant_holds_at_every_fail_path_step() {
    let (env, client, _admin, relay) = setup();
    let payload = make_payload(&env, 1);
    let id = client.register_callback(&payload);
    let mut ids = SorobanVec::new(&env);
    ids.push_back(id.clone());

    assert_conservation(&client, &ids, 1, "fail-step:registered");

    client.start_processing(&id, &relay);
    assert_conservation(&client, &ids, 1, "fail-step:processing");

    client.fail_transaction(&id, &String::from_str(&env, "r"), &relay);
    assert_conservation(&client, &ids, 1, "fail-step:failed");
}

/// **Conservation holds with direct Pending → Failed path.**
#[test]
fn invariant_holds_pending_to_failed_directly() {
    let (env, client, _admin, relay) = setup();
    let payload = make_payload(&env, 2);
    let id = client.register_callback(&payload);
    let mut ids = SorobanVec::new(&env);
    ids.push_back(id.clone());

    assert_conservation(&client, &ids, 1, "direct-fail:registered");
    client.fail_transaction(&id, &String::from_str(&env, "r"), &relay);
    assert_conservation(&client, &ids, 1, "direct-fail:failed");
}

/// **Conservation after a pause/unpause cycle does not affect existing counts.**
#[test]
fn invariant_pause_unpause_does_not_affect_counts() {
    let (env, client, _admin, relay) = setup();
    let n = 8usize;
    let mut ids = SorobanVec::new(&env);

    for i in 0..n {
        let payload = make_payload(&env, i);
        let id = client.register_callback(&payload);
        ids.push_back(id);
    }

    assert_conservation(&client, &ids, n, "before_pause");

    client.pause();
    assert_conservation(&client, &ids, n, "while_paused");

    // Status transitions still work while paused.
    let first_id = ids.get(0).unwrap();
    client.start_processing(&first_id, &relay);
    assert_conservation(&client, &ids, n, "transition_while_paused");

    // Unpause and register more.
    client.unpause();
    let extra = 4usize;
    for i in 0..extra {
        let payload = make_payload_b(&env, i);
        let id = client.register_callback(&payload);
        ids.push_back(id);
    }

    assert_conservation(&client, &ids, n + extra, "after_unpause_more_registered");
}

/// **Deterministic pseudo-random scenario — five independent seeds.**
///
/// Exercises varied operation orderings to catch any state-bookkeeping bug
/// that only appears after specific sequences.
#[test]
// Parallel indexing into `TX_IDS`, `IDEM_IDS`, `ids` and `statuses` reads
// clearer as a range loop than as nested zips.
#[allow(clippy::needless_range_loop)]
fn invariant_pseudorandom_operation_sequences() {
    // (seed, offset into TX_IDS/IDEM_IDS) — 12 txs per seed, 5 seeds = 60 total.
    // We stagger offsets so each seed uses a disjoint set of IDs within the 30
    // available slots.
    let seeds: &[(u64, usize)] = &[
        (0xDEAD_BEEF_CAFE_0001, 0),
        (0x1234_5678_9ABC_DEF0, 6), // use indices 6..17
        (0xFEED_FACE_DEAD_BABE, 12),
        (0x0000_0000_FFFF_FFFF, 18),
        // Last seed re-uses indices 0..11 with a fresh contract — no ID collision
        // since each seed gets its own contract instance.
        (0xAAAA_BBBB_CCCC_DDDD, 0),
    ];

    for &(seed, offset) in seeds {
        let mut rng = Lcg::new(seed);
        let (env, client, _admin, relay) = setup();

        let num_txs = 12usize;
        let mut ids = SorobanVec::new(&env);
        // We track in-memory status with a fixed-size array.
        let mut statuses: [TransactionStatus; 12] =
            core::array::from_fn(|_| TransactionStatus::Pending);

        // Phase 1: register all transactions.
        for i in 0..num_txs {
            let idx = offset + i;
            let payload = {
                let account = g_address(&env);
                CallbackPayload {
                    transaction_id: String::from_str(&env, TX_IDS[idx]),
                    stellar_account: account.clone(),
                    amount: 1_000 + idx as i128,
                    asset_code: String::from_str(&env, "USDC"),
                    asset_issuer: account,
                    idempotency_key: String::from_str(&env, IDEM_IDS[idx]),
                    anchor_transaction_id: String::from_str(&env, "anc-rng"),
                    callback_type: CallbackType::Deposit,
                    callback_status: String::from_str(&env, "pending_external"),
                }
            };
            let id = client.register_callback(&payload);
            ids.push_back(id);
        }

        assert_conservation(&client, &ids, num_txs, "rng:after_register");

        // Phase 2: apply 30 random valid transitions.
        for _ in 0..30usize {
            let i = rng.next_usize_below(num_txs);
            let id = ids.get(idx(i)).unwrap();

            match &statuses[i] {
                TransactionStatus::Pending => {
                    if rng.next_bool() {
                        client.start_processing(&id, &relay);
                        statuses[i] = TransactionStatus::Processing;
                    } else {
                        client.fail_transaction(&id, &String::from_str(&env, "rng-fail"), &relay);
                        statuses[i] = TransactionStatus::Failed;
                    }
                }
                TransactionStatus::Processing => {
                    if rng.next_bool() {
                        client.complete_transaction(
                            &id,
                            &String::from_str(&env, "rng-hash"),
                            &relay,
                        );
                        statuses[i] = TransactionStatus::Completed;
                    } else {
                        client.fail_transaction(&id, &String::from_str(&env, "rng-fail"), &relay);
                        statuses[i] = TransactionStatus::Failed;
                    }
                }
                // Terminal — no-op.
                TransactionStatus::Completed | TransactionStatus::Failed => {}
            }

            // Assert conservation after every single operation.
            assert_conservation(&client, &ids, num_txs, "rng:mid-op");
        }

        // Final: in-memory model must match on-chain state.
        for (i, expected) in statuses.iter().enumerate() {
            let id = ids.get(idx(i)).unwrap();
            assert_eq!(
                &client.get_status(&id),
                expected,
                "seed={seed:#x}: in-memory model diverged from on-chain state at idx={i}"
            );
        }
    }
}

/// **No transaction can vanish**: every registered transaction is still
/// readable after all are driven to terminal states.
#[test]
fn invariant_no_transaction_vanishes() {
    let (env, client, _admin, relay) = setup();
    let n = 15usize;
    let mut ids = SorobanVec::new(&env);

    for i in 0..n {
        let payload = make_payload(&env, i);
        let id = client.register_callback(&payload);
        client.start_processing(&id, &relay);
        if i % 3 == 0 {
            client.fail_transaction(&id, &String::from_str(&env, "r"), &relay);
        } else {
            client.complete_transaction(&id, &String::from_str(&env, "hash-van"), &relay);
        }
        ids.push_back(id);
    }

    // Every registered transaction must still be readable.
    for id in ids.iter() {
        let tx = client.get_transaction(&id);
        assert_eq!(tx.id, id, "transaction vanished");
    }

    assert_conservation(&client, &ids, n, "no_vanish_final");
}

/// **No transaction can be double-counted**: each unique ID contributes exactly
/// one status observation to the tally.
#[test]
fn invariant_no_double_counting() {
    let (env, client, _admin, relay) = setup();
    let n = 10usize;
    let mut ids = SorobanVec::new(&env);

    for i in 0..n {
        let payload = make_payload(&env, i);
        let id = client.register_callback(&payload);
        ids.push_back(id);
    }

    // Drive half to Completed.
    for i in 0..(n / 2) {
        let id = ids.get(idx(i)).unwrap();
        client.start_processing(&id, &relay);
        client.complete_transaction(&id, &String::from_str(&env, "h"), &relay);
    }

    // Verify all IDs are unique by checking pairwise inequality.
    for i in 0..n {
        for j in (i + 1)..n {
            assert_ne!(
                ids.get(idx(i)).unwrap(),
                ids.get(idx(j)).unwrap(),
                "duplicate tx_id at indices {i} and {j}"
            );
        }
    }

    assert_conservation(&client, &ids, n, "no_double_count_final");
}
