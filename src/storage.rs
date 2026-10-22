//! Storage helpers for the anchor registry contract.
//!
//! This module centralises all key definitions and read/write helpers so that
//! validation logic (see `validation.rs`) and the contract entry points share a
//! single, consistent view of persistent and instance storage.
//!
//! # Amount ceilings
//!
//! Two layers of amount ceilings are supported:
//!
//! * A **global default ceiling** (`global_max_amount`) stored in `instance`
//!   storage. It is admin-settable and, when unset, falls back to
//!   [`DEFAULT_GLOBAL_MAX_AMOUNT`]. An unset global ceiling never means "no
//!   ceiling" — it always resolves to the documented default.
//! * A **per-anchor override** stored in `persistent` storage keyed by the
//!   anchor address. When an anchor has no explicit override, the global
//!   default ceiling applies.

use soroban_sdk::{contracttype, Address, Env};

/// Default global maximum transaction amount used when the admin has never
/// explicitly configured a global ceiling.
///
/// This value is deliberately conservative: an unset ceiling must never be
/// interpreted as "no ceiling", which would silently disable amount validation
/// for every anchor without an override.
pub const DEFAULT_GLOBAL_MAX_AMOUNT: i128 = 1_000_000_000_000;

/// Storage keys used by the contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageKey {
    /// Instance-tier key holding the admin-settable global default ceiling.
    GlobalMaxAmount,
    /// Persistent-tier key holding a per-anchor amount-ceiling override.
    AnchorAmountCeiling(Address),
}

// ---------------------------------------------------------------------------
// Global default ceiling (instance storage)
// ---------------------------------------------------------------------------

/// Returns the configured global default amount ceiling.
///
/// If the admin has never set a global ceiling, [`DEFAULT_GLOBAL_MAX_AMOUNT`]
/// is returned. This function never returns `None`; an unset ceiling always
/// resolves to the documented default rather than "no ceiling".
pub fn get_global_max_amount(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&StorageKey::GlobalMaxAmount)
        .unwrap_or(DEFAULT_GLOBAL_MAX_AMOUNT)
}

/// Sets the global default amount ceiling.
///
/// # Panics
///
/// Panics if `amount` is not strictly positive. A non-positive ceiling would
/// either reject every transaction or be meaningless, so it is rejected at the
/// storage boundary.
pub fn set_global_max_amount(env: &Env, amount: i128) {
    assert!(amount > 0, "global max amount must be positive");
    env.storage()
        .instance()
        .set(&StorageKey::GlobalMaxAmount, &amount);
}

/// Returns `true` if the admin has explicitly configured a global ceiling.
pub fn has_global_max_amount(env: &Env) -> bool {
    env.storage().instance().has(&StorageKey::GlobalMaxAmount)
}

// ---------------------------------------------------------------------------
// Per-anchor override (persistent storage)
// ---------------------------------------------------------------------------

/// Returns the per-anchor amount-ceiling override, if one is set.
pub fn get_anchor_amount_ceiling(env: &Env, anchor: &Address) -> Option<i128> {
    env.storage()
        .persistent()
        .get(&StorageKey::AnchorAmountCeiling(anchor.clone()))
}

/// Sets a per-anchor amount-ceiling override.
///
/// # Panics
///
/// Panics if `amount` is not strictly positive.
pub fn set_anchor_amount_ceiling(env: &Env, anchor: &Address, amount: i128) {
    assert!(amount > 0, "anchor amount ceiling must be positive");
    env.storage()
        .persistent()
        .set(&StorageKey::AnchorAmountCeiling(anchor.clone()), &amount);
}

/// Removes a per-anchor amount-ceiling override, causing the anchor to fall
/// back to the global default ceiling.
pub fn remove_anchor_amount_ceiling(env: &Env, anchor: &Address) {
    env.storage()
        .persistent()
        .remove(&StorageKey::AnchorAmountCeiling(anchor.clone()));
}

/// Returns the effective amount ceiling for `anchor`.
///
/// Resolution order:
/// 1. The per-anchor override, if set.
/// 2. Otherwise, the global default ceiling (which itself falls back to
///    [`DEFAULT_GLOBAL_MAX_AMOUNT`] when unset).
///
/// This function never returns `None`: every anchor always has a ceiling.
pub fn get_effective_amount_ceiling(env: &Env, anchor: &Address) -> i128 {
    get_anchor_amount_ceiling(env, anchor).unwrap_or_else(|| get_global_max_amount(env))
}