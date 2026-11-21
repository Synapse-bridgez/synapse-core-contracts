// src/events.rs - Issues #250, #252, #251, #249
// Event restoration for upgrade lifecycle, bonding/slashing, relay, and self-check

#![no_std]

use soroban_sdk::{Env, Symbol, Address, symbol_short};

// ============================================================================
// Issue #250: Upgrade-Lifecycle Events
// ============================================================================

pub fn emit_upgrade_proposed(env: &Env, proposer: &Address, wasm_hash: &[u8; 32]) {
    env.events().publish(
        (symbol_short!("upgrade"), symbol_short!("propose")),
        (proposer, wasm_hash),
    );
}

pub fn emit_upgrade_finalized(env: &Env, executor: &Address, wasm_hash: &[u8; 32]) {
    env.events().publish(
        (symbol_short!("upgrade"), symbol_short!("finalize")),
        (executor, wasm_hash),
    );
}

pub fn emit_upgrade_cancelled(env: &Env, canceller: &Address) {
    env.events().publish(
        (symbol_short!("upgrade"), symbol_short!("cancel")),
        canceller,
    );
}

pub fn emit_upgrade_rollback(env: &Env, executor: &Address) {
    env.events().publish(
        (symbol_short!("upgrade"), symbol_short!("rollback")),
        executor,
    );
}

// ============================================================================
// Issue #252: Collateral Bonding/Slashing Events
// ============================================================================

pub fn emit_bond(env: &Env, bonder: &Address, amount: i128, collateral_id: u32) {
    env.events().publish(
        (symbol_short!("bond"),),
        (bonder, amount, collateral_id),
    );
}

pub fn emit_unbond_requested(env: &Env, unbonder: &Address, amount: i128, collateral_id: u32) {
    env.events().publish(
        (symbol_short!("unbond-req"),),
        (unbonder, amount, collateral_id),
    );
}

pub fn emit_unbond_claimed(env: &Env, claimer: &Address, amount: i128, collateral_id: u32) {
    env.events().publish(
        (symbol_short!("unbond-claim"),),
        (claimer, amount, collateral_id),
    );
}

pub fn emit_slash(env: &Env, slashed: &Address, amount: i128, reason: &Symbol) {
    env.events().publish(
        (symbol_short!("slash"),),
        (slashed, amount, reason),
    );
}

// ============================================================================
// Issue #251: Relay-Signer-Set Change Events
// ============================================================================

pub fn emit_relay_signer_rotated(env: &Env, old_signer: &Address, new_signer: &Address) {
    env.events().publish(
        (symbol_short!("relay"),),
        (symbol_short!("rotate"), old_signer, new_signer),
    );
}

pub fn emit_relay_signer_added(env: &Env, signer: &Address, threshold: u32) {
    env.events().publish(
        (symbol_short!("relay"),),
        (symbol_short!("add"), signer, threshold),
    );
}

pub fn emit_relay_signer_removed(env: &Env, signer: &Address, threshold: u32) {
    env.events().publish(
        (symbol_short!("relay"),),
        (symbol_short!("remove"), signer, threshold),
    );
}

pub fn emit_relay_threshold_changed(env: &Env, old_threshold: u32, new_threshold: u32) {
    env.events().publish(
        (symbol_short!("relay"),),
        (symbol_short!("threshold"), old_threshold, new_threshold),
    );
}

// ============================================================================
// Issue #249: Upgrade Self-Check Events
// ============================================================================

pub fn emit_self_check_passed(env: &Env, check_id: &Symbol, details: &Symbol) {
    env.events().publish(
        (symbol_short!("self-check"), symbol_short!("passed")),
        (check_id, details),
    );
}

pub fn emit_self_check_failed(env: &Env, check_id: &Symbol, reason: &Symbol) {
    env.events().publish(
        (symbol_short!("self-check"), symbol_short!("failed")),
        (check_id, reason),
    );
}

// ============================================================================
// Helper: Emit all events from post-upgrade self-check
// ============================================================================

pub fn emit_post_upgrade_check_events(env: &Env, success: bool) {
    if success {
        emit_self_check_passed(
            env,
            &symbol_short!("storage"),
            &symbol_short!("ok"),
        );
        emit_self_check_passed(
            env,
            &symbol_short!("signers"),
            &symbol_short!("ok"),
        );
    } else {
        emit_self_check_failed(
            env,
            &symbol_short!("storage"),
            &symbol_short!("corrupt"),
        );
    }
}
