//! # Relay-signer key-rotation × in-flight transaction harness (#132)
//!
//! Explores the interleavings between `set_relay_signer` rotation and the
//! transaction lifecycle (`register_callback` → `start_processing` →
//! `complete_transaction` | `fail_transaction`), and asserts two properties:
//!
//! 1. **Safety**: a rotated-out ("stale") signer can never register or
//!    advance anything, and a rejected attempt leaves state untouched.
//! 2. **Liveness**: no in-flight transaction ever becomes stuck. Whatever the
//!    rotation timing, the *current* relay signer and the admin can always
//!    drive every `Pending`/`Processing` transaction to a terminal state.
//!
//! ## Structure
//!
//! * **Exhaustive enumeration** (`exhaustive_rotation_interleavings_*`):
//!   for each lifecycle path, every subset of rotation points (before
//!   registration, between each step, after the last step) × every rotation
//!   target kind × two driver patterns. Every stale signer probes each step
//!   before the legitimate driver runs it.
//! * **Named scenarios**: the timings the issue calls out explicitly
//!   (rotation before registration, during processing, between processing and
//!   completion) plus several transactions in flight at once.
//! * **Seeded chaos** ([`seeded_chaos_against_model`]): long pseudo-random
//!   runs mixing rotations, admin transfers, pause toggles and lifecycle
//!   calls from every kind of actor, each checked against a reference model,
//!   followed by a drain phase that proves liveness.
//!
//! ## Why liveness holds today, and what would break it
//!
//! The contract authorises transitions against the relay signer *stored at
//! call time* (`AdminClient::assert_is_relay_or_admin`), and nothing binds a
//! transaction to the signer that registered it. Rotation therefore can't
//! orphan an in-flight transaction. The N-of-M signer-set, timelocked
//! rotation and reassignment-recovery entry points this harness is meant to
//! cover alongside have not landed on `main` (see #199). When any of them
//! introduces a per-transaction signer binding, extend [`Actor`] and the
//! model's `authorised` rule here. A transaction the drain phase can't
//! finish must then be treated as a bug, not a known limitation.

#![cfg(test)]

extern crate std;

use std::{format, vec, vec::Vec};

use soroban_sdk::{
    testutils::{Address as _, EnvTestConfig, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, String,
};

use crate::types::{CallbackPayload, CallbackType, ContractError, TransactionStatus};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

const G_ADDR: &str = "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ";

// ─── Harness plumbing ────────────────────────────────────────────────────────

struct World {
    env: Env,
    client: SynapseCoreContractClient<'static>,
    admin: Address,
    relay: Address,
    /// Every relay signer that has been rotated out and is not current.
    stale_relays: Vec<Address>,
    /// Every relay signer that has ever been current, in order.
    relay_history: Vec<Address>,
    next_tx: u32,
}

impl World {
    fn new() -> Self {
        // Hundreds of Envs per test: skip the SDK's JSON snapshot-at-drop,
        // which otherwise dominates the runtime and writes one file per Env.
        let env = Env::new_with_config(EnvTestConfig {
            capture_snapshot_at_drop: false,
        });
        let id = env.register(SynapseCoreContract, ());
        let client = SynapseCoreContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        let relay = Address::generate(&env);
        env.mock_all_auths();
        client.initialize(&admin, &relay);
        World {
            env,
            client,
            admin,
            relay: relay.clone(),
            stale_relays: Vec::new(),
            relay_history: vec![relay],
            next_tx: 0,
        }
    }

    fn payload(&mut self) -> CallbackPayload {
        self.next_tx += 1;
        let id = format!("tx-{}", self.next_tx);
        let acct = String::from_str(&self.env, G_ADDR);
        CallbackPayload {
            transaction_id: String::from_str(&self.env, &id),
            stellar_account: acct.clone(),
            amount: 1_000,
            asset_code: String::from_str(&self.env, "USDC"),
            asset_issuer: acct,
            idempotency_key: String::from_str(&self.env, &id),
            anchor_transaction_id: String::from_str(&self.env, "anchor"),
            callback_type: CallbackType::Deposit,
            callback_status: String::from_str(&self.env, "pending_external"),
        }
    }

    /// Rotate to `next`, keeping the stale set accurate (a signer rotated
    /// back in is no longer stale; the admin is never "stale").
    fn rotate_to(&mut self, next: Address) {
        self.client.set_relay_signer(&next);
        let old = core::mem::replace(&mut self.relay, next.clone());
        if old != next && !self.stale_relays.contains(&old) {
            self.stale_relays.push(old);
        }
        self.stale_relays.retain(|a| *a != next && *a != self.admin);
        self.relay_history.push(next);
        assert_eq!(self.client.relay_signer(), self.relay);
    }

    fn rotate(&mut self, kind: RotationKind) {
        let next = match kind {
            RotationKind::Fresh => Address::generate(&self.env),
            RotationKind::SameSigner => self.relay.clone(),
            RotationKind::ToAdmin => self.admin.clone(),
            RotationKind::BackToOriginal => {
                if self.relay == self.relay_history[0] {
                    Address::generate(&self.env)
                } else {
                    self.relay_history[0].clone()
                }
            }
        };
        self.rotate_to(next);
    }

    /// Register with **only** `signer`'s authorisation mocked, so a stale
    /// signer cannot piggy-back on a blanket `mock_all_auths`.
    fn register_as(&self, signer: &Address, p: &CallbackPayload) -> bool {
        let args = (p.clone(),).into_val(&self.env);
        let ok = self
            .client
            .mock_auths(&[MockAuth {
                address: signer,
                invoke: &MockAuthInvoke {
                    contract: &self.client.address,
                    fn_name: "register_callback",
                    args,
                    sub_invokes: &[],
                },
            }])
            .try_register_callback(p)
            .is_ok();
        // Restore blanket mocking for the rest of the scenario.
        self.env.mock_all_auths();
        ok
    }

    fn status(&self, id: &String) -> TransactionStatus {
        self.client.get_status(id)
    }

    fn step(&self, op: Op, id: &String, caller: &Address) -> Result<(), ContractError> {
        let r = match op {
            Op::Start => self.client.try_start_processing(id, caller),
            Op::Complete => self.client.try_complete_transaction(
                id,
                &String::from_str(&self.env, "hash"),
                caller,
            ),
            Op::Fail => {
                self.client
                    .try_fail_transaction(id, &String::from_str(&self.env, "err"), caller)
            }
        };
        match r {
            Ok(Ok(())) => Ok(()),
            Err(Ok(e)) => Err(e),
            other => panic!("unexpected host-level result: {other:?}"),
        }
    }

    /// Every stale signer attempts `op`; each must be rejected with
    /// `Unauthorised` and leave the transaction's status unchanged.
    fn stale_signers_cannot(&self, op: Op, id: &String) {
        let before = self.status(id);
        for stale in &self.stale_relays {
            assert_eq!(
                self.step(op, id, stale),
                Err(ContractError::Unauthorised),
                "stale signer must not be able to {op:?}"
            );
            assert_eq!(self.status(id), before, "rejected {op:?} mutated state");
        }
    }

    /// Liveness: from any non-terminal state the current relay can finish
    /// the transaction.
    fn assert_can_finish(&self, id: &String) {
        match self.status(id) {
            TransactionStatus::Pending => {
                assert_eq!(self.step(Op::Start, id, &self.relay), Ok(()));
                assert_eq!(self.step(Op::Complete, id, &self.relay), Ok(()));
            }
            TransactionStatus::Processing => {
                assert_eq!(self.step(Op::Complete, id, &self.relay), Ok(()));
            }
            TransactionStatus::Completed | TransactionStatus::Failed => {}
        }
        assert!(matches!(
            self.status(id),
            TransactionStatus::Completed | TransactionStatus::Failed
        ));
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Start,
    Complete,
    Fail,
}

#[derive(Clone, Copy, Debug)]
enum RotationKind {
    /// Rotate to a never-seen key (the normal operational rotation).
    Fresh,
    /// "Rotate" to the key that is already current (a no-op rotation).
    SameSigner,
    /// Rotate so the relay and admin roles share one key.
    ToAdmin,
    /// Roll back to the original key (A → … → A).
    BackToOriginal,
}

const ROTATION_KINDS: [RotationKind; 4] = [
    RotationKind::Fresh,
    RotationKind::SameSigner,
    RotationKind::ToAdmin,
    RotationKind::BackToOriginal,
];

const PATHS: [&[Op]; 3] = [
    &[Op::Start, Op::Complete],
    &[Op::Start, Op::Fail],
    &[Op::Fail],
];

// ─── Exhaustive enumeration ─────────────────────────────────────────────────

/// Every rotation-timing × rotation-kind × driver combination for one
/// lifecycle path. Rotation slot `0` is *before registration*; slot `k`
/// (`1..=n`) is *after step `k`*. That covers after registration, between
/// processing and completion, and after the terminal step. Returns the
/// number of scenarios run.
fn exhaustive_rotation_interleavings(path: &[Op]) -> u32 {
    let mut scenarios = 0u32;
    let slots = path.len() + 2; // before reg, after reg, after each op
    for mask in 0u32..(1 << slots) {
        for kind in ROTATION_KINDS {
            run_interleaving(path, mask, kind, false);
            scenarios += 1;
        }
        // Driver dimension: the admin (not the relay) drives every odd step.
        // The rotation kind does not interact with who drives, so one kind
        // suffices here.
        run_interleaving(path, mask, RotationKind::Fresh, true);
        scenarios += 1;
    }
    scenarios
}

// One test per path so the enumeration runs in parallel.

#[test]
fn exhaustive_rotation_interleavings_start_complete() {
    assert_eq!(exhaustive_rotation_interleavings(PATHS[0]), 16 * 5);
}

#[test]
fn exhaustive_rotation_interleavings_start_fail() {
    assert_eq!(exhaustive_rotation_interleavings(PATHS[1]), 16 * 5);
}

#[test]
fn exhaustive_rotation_interleavings_fail_from_pending() {
    assert_eq!(exhaustive_rotation_interleavings(PATHS[2]), 8 * 5);
}

fn run_interleaving(path: &[Op], mask: u32, kind: RotationKind, admin_drives_odd_steps: bool) {
    let mut w = World::new();
    let rotate_at = |slot: usize| mask & (1 << slot) != 0;

    // Slot 0: rotation *before* registration.
    if rotate_at(0) {
        w.rotate(kind);
    }
    let p = w.payload();
    for stale in w.stale_relays.clone() {
        assert!(
            !w.register_as(&stale, &p),
            "stale signer registered a callback"
        );
    }
    assert!(
        w.register_as(&w.relay.clone(), &p),
        "current signer must register"
    );
    let id = p.transaction_id.clone();
    assert_eq!(w.status(&id), TransactionStatus::Pending);

    for (i, &op) in path.iter().enumerate() {
        // Slot i+1: rotation after the previous step, before this one.
        if rotate_at(i + 1) {
            w.rotate(kind);
        }
        w.stale_signers_cannot(op, &id);
        let driver = if admin_drives_odd_steps && i % 2 == 1 {
            w.admin.clone()
        } else {
            w.relay.clone()
        };
        assert_eq!(
            w.step(op, &id, &driver),
            Ok(()),
            "path {path:?} mask {mask:#b} kind {kind:?}: step {op:?} blocked"
        );
    }

    // Final slot: rotation after the terminal step must not disturb it.
    let terminal = w.status(&id);
    if rotate_at(path.len() + 1) {
        w.rotate(kind);
    }
    assert_eq!(w.status(&id), terminal);
    let expected = if *path.last().unwrap() == Op::Complete {
        TransactionStatus::Completed
    } else {
        TransactionStatus::Failed
    };
    assert_eq!(terminal, expected);
    for stale in &w.stale_relays {
        assert_eq!(
            w.step(Op::Fail, &id, stale),
            Err(ContractError::Unauthorised)
        );
    }
}

// ─── Named scenarios ────────────────────────────────────────────────────────

#[test]
fn rotation_before_registration_rejects_old_signer_and_accepts_new() {
    let mut w = World::new();
    let old = w.relay.clone();
    w.rotate(RotationKind::Fresh);
    let p = w.payload();
    assert!(!w.register_as(&old, &p));
    assert!(!w.client.try_get_transaction(&p.transaction_id).is_ok());
    assert!(w.register_as(&w.relay.clone(), &p));
    w.assert_can_finish(&p.transaction_id);
}

#[test]
fn rotation_while_pending_hands_the_transaction_to_the_new_signer() {
    let mut w = World::new();
    let p = w.payload();
    let id = w.client.register_callback(&p);
    let old = w.relay.clone();
    w.rotate(RotationKind::Fresh);
    assert_eq!(
        w.step(Op::Start, &id, &old),
        Err(ContractError::Unauthorised)
    );
    assert_eq!(w.status(&id), TransactionStatus::Pending);
    w.assert_can_finish(&id);
}

#[test]
fn rotation_between_processing_and_completion() {
    let mut w = World::new();
    let p = w.payload();
    let id = w.client.register_callback(&p);
    w.client.start_processing(&id, &w.relay);
    let old = w.relay.clone();
    w.rotate(RotationKind::Fresh);
    assert_eq!(
        w.step(Op::Complete, &id, &old),
        Err(ContractError::Unauthorised)
    );
    assert_eq!(
        w.step(Op::Fail, &id, &old),
        Err(ContractError::Unauthorised)
    );
    assert_eq!(w.status(&id), TransactionStatus::Processing);
    assert_eq!(w.step(Op::Complete, &id, &w.relay), Ok(()));
}

/// Registration authorises against the relay stored *at call time*: every
/// successful registration is signed by the current key, never a stale one.
#[test]
fn registration_auth_always_names_the_current_signer() {
    let mut w = World::new();
    for kind in ROTATION_KINDS {
        w.rotate(kind);
        let p = w.payload();
        w.client.register_callback(&p);
        let auths = w.env.auths();
        assert_eq!(auths.len(), 1);
        assert_eq!(auths[0].0, w.relay, "registration authorised by {kind:?}");
    }
}

#[test]
fn many_transactions_in_flight_across_repeated_rotations() {
    let mut w = World::new();
    let mut ids = Vec::new();
    for stage in 0..9u32 {
        let p = w.payload();
        let id = w.client.register_callback(&p);
        match stage % 3 {
            0 => {}
            1 => w.client.start_processing(&id, &w.relay),
            _ => {
                w.client.start_processing(&id, &w.relay);
                w.client
                    .complete_transaction(&id, &String::from_str(&w.env, "h"), &w.relay);
            }
        }
        ids.push(id);
        w.rotate(ROTATION_KINDS[(stage % 4) as usize]);
    }
    for id in &ids {
        w.stale_signers_cannot(Op::Fail, id);
        w.assert_can_finish(id);
    }
}

/// An admin transfer interleaved with relay rotation: the outgoing admin
/// loses its ability to drive transactions, the incoming one gains it, and
/// the current relay is unaffected throughout.
#[test]
fn admin_transfer_interleaved_with_rotation() {
    let mut w = World::new();
    let p = w.payload();
    let id = w.client.register_callback(&p);
    w.rotate(RotationKind::Fresh);
    let old_admin = w.admin.clone();
    let new_admin = Address::generate(&w.env);
    w.client.propose_admin(&new_admin);
    // Mid-transfer the old admin is still in charge.
    assert_eq!(w.step(Op::Start, &id, &old_admin), Ok(()));
    w.client.accept_admin(&new_admin);
    w.admin = new_admin.clone();
    assert_eq!(
        w.step(Op::Complete, &id, &old_admin),
        Err(ContractError::Unauthorised)
    );
    w.rotate(RotationKind::Fresh);
    assert_eq!(w.step(Op::Complete, &id, &new_admin), Ok(()));
}

/// Pause blocks ingestion only; rotation during a pause must still let the
/// new signer drain in-flight work.
#[test]
fn rotation_during_pause_still_drains() {
    let mut w = World::new();
    let p = w.payload();
    let id = w.client.register_callback(&p);
    w.client.pause();
    w.rotate(RotationKind::Fresh);
    let p2 = w.payload();
    assert_eq!(
        w.client.try_register_callback(&p2),
        Err(Ok(ContractError::ContractPaused))
    );
    w.assert_can_finish(&id);
}

// ─── Seeded chaos against a reference model ─────────────────────────────────

/// Tiny deterministic PRNG (64-bit LCG, Knuth MMIX constants).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[derive(Clone, Copy, Debug)]
enum Actor {
    CurrentRelay,
    Admin,
    StaleRelay,
    StaleAdmin,
    Stranger,
}

fn model_transition(from: &TransactionStatus, op: Op) -> Option<TransactionStatus> {
    use TransactionStatus::*;
    match (from, op) {
        (Pending, Op::Start) => Some(Processing),
        (Processing, Op::Complete) => Some(Completed),
        (Pending | Processing, Op::Fail) => Some(Failed),
        _ => None,
    }
}

/// Default depth keeps `cargo test` (and every cargo-mutants run) fast.
/// Deeper local runs: `SYNAPSE_CHAOS_SEEDS=64 SYNAPSE_CHAOS_STEPS=1000
/// cargo test --release seeded_chaos`.
#[test]
fn seeded_chaos_against_model() {
    let knob = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let seeds = knob("SYNAPSE_CHAOS_SEEDS", 4);
    let steps = knob("SYNAPSE_CHAOS_STEPS", 150) as u32;
    for seed in 1..=seeds {
        run_chaos(seed, steps);
    }
}

fn run_chaos(seed: u64, steps: u32) {
    let mut rng = Lcg(seed);
    let mut w = World::new();
    let mut stale_admins: Vec<Address> = Vec::new();
    let mut paused = false;
    // Model: (tx id, expected status)
    let mut txs: Vec<(String, TransactionStatus)> = Vec::new();

    for step in 0..steps {
        let ctx = format!("seed {seed} step {step}");
        match rng.below(10) {
            // Rotation, any kind.
            0 | 1 => w.rotate(ROTATION_KINDS[rng.below(4)]),
            // Admin transfer.
            2 => {
                let next = Address::generate(&w.env);
                w.client.propose_admin(&next);
                w.client.accept_admin(&next);
                let old = core::mem::replace(&mut w.admin, next);
                if old != w.relay {
                    stale_admins.push(old);
                }
            }
            // Pause toggle.
            3 => {
                if paused {
                    w.client.unpause();
                } else {
                    w.client.pause();
                }
                paused = !paused;
            }
            // New registration by the current relay.
            4 => {
                let p = w.payload();
                let r = w.client.try_register_callback(&p);
                if paused {
                    assert_eq!(r, Err(Ok(ContractError::ContractPaused)), "{ctx}");
                } else {
                    assert!(r.is_ok(), "{ctx}: current relay registration failed");
                    txs.push((p.transaction_id, TransactionStatus::Pending));
                }
            }
            // Lifecycle call from a random actor on a random tx.
            _ => {
                if txs.is_empty() {
                    continue;
                }
                let i = rng.below(txs.len());
                let op = [Op::Start, Op::Complete, Op::Fail][rng.below(3)];
                let actor = [
                    Actor::CurrentRelay,
                    Actor::Admin,
                    Actor::StaleRelay,
                    Actor::StaleAdmin,
                    Actor::Stranger,
                ][rng.below(5)];
                let caller = match actor {
                    Actor::CurrentRelay => w.relay.clone(),
                    Actor::Admin => w.admin.clone(),
                    Actor::StaleRelay if !w.stale_relays.is_empty() => {
                        w.stale_relays[rng.below(w.stale_relays.len())].clone()
                    }
                    Actor::StaleAdmin if !stale_admins.is_empty() => {
                        stale_admins[rng.below(stale_admins.len())].clone()
                    }
                    _ => Address::generate(&w.env),
                };
                let authorised = caller == w.relay || caller == w.admin;
                let expected = if !authorised {
                    Err(ContractError::Unauthorised)
                } else {
                    match model_transition(&txs[i].1, op) {
                        Some(next) => {
                            txs[i].1 = next;
                            Ok(())
                        }
                        None => Err(ContractError::InvalidStatusTransition),
                    }
                };
                let got = w.step(op, &txs[i].0, &caller);
                assert_eq!(got, expected, "{ctx}: {actor:?} {op:?} on {:?}", txs[i].0);
            }
        }
        // State always matches the model.
        for (id, status) in &txs {
            assert_eq!(&w.status(id), status, "{ctx}: model/contract diverged");
        }
    }

    // Drain: liveness under whatever rotation/admin state chaos left behind.
    for (id, _) in &txs {
        w.stale_signers_cannot(Op::Fail, id);
        w.assert_can_finish(id);
    }
}
