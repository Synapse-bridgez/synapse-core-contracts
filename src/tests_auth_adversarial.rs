//! Adversarial auth tests (issue #483).
//!
//! Verifies that each admin guard in `src/admin.rs` fails with its **own**
//! distinct error code, so a caller can tell exactly which check rejected the
//! call without inspecting logs or events.
//!
//! | Guard                    | Expected error on failure              |
//! |--------------------------|----------------------------------------|
//! | `require_admin`          | `ContractError::NotAdmin`              |
//! | `require_relay_or_admin` | `ContractError::NotRelayOrAdmin`       |
//! | `require_pending_admin`  | `ContractError::NotPendingAdmin`       |
//! | `require_guardian`       | `ContractError::NotGuardian`           |

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};

use crate::types::ContractError;
use crate::admin;

// ─── require_admin ────────────────────────────────────────────────────────────

#[test]
fn test_require_admin_returns_not_admin_on_wrong_caller() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let caller = Address::generate(&env);

    let result = admin::require_admin(&env, &caller, &admin);
    assert_eq!(result, Err(ContractError::NotAdmin));
}

#[test]
fn test_require_admin_succeeds_for_correct_caller() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    assert_eq!(admin::require_admin(&env, &admin, &admin), Ok(()));
}

// ─── require_relay_or_admin ───────────────────────────────────────────────────

#[test]
fn test_require_relay_or_admin_returns_not_relay_or_admin_for_bystander() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    let bystander = Address::generate(&env);

    let result = admin::require_relay_or_admin(&env, &bystander, &admin, &relay);
    assert_eq!(result, Err(ContractError::NotRelayOrAdmin));
}

#[test]
fn test_require_relay_or_admin_succeeds_for_admin() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let relay = Address::generate(&env);

    assert_eq!(admin::require_relay_or_admin(&env, &admin, &admin, &relay), Ok(()));
}

#[test]
fn test_require_relay_or_admin_succeeds_for_relay() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let relay = Address::generate(&env);

    assert_eq!(admin::require_relay_or_admin(&env, &relay, &admin, &relay), Ok(()));
}

// ─── require_pending_admin ────────────────────────────────────────────────────

#[test]
fn test_require_pending_admin_returns_not_pending_admin_for_wrong_caller() {
    let env = Env::default();
    env.mock_all_auths();

    let pending = Address::generate(&env);
    let caller = Address::generate(&env);

    let result = admin::require_pending_admin(&env, &caller, &pending);
    assert_eq!(result, Err(ContractError::NotPendingAdmin));
}

#[test]
fn test_require_pending_admin_succeeds_for_correct_caller() {
    let env = Env::default();
    env.mock_all_auths();

    let pending = Address::generate(&env);
    assert_eq!(admin::require_pending_admin(&env, &pending, &pending), Ok(()));
}

// ─── require_guardian ─────────────────────────────────────────────────────────

#[test]
fn test_require_guardian_returns_not_guardian_for_wrong_caller() {
    let env = Env::default();
    env.mock_all_auths();

    let guardian = Address::generate(&env);
    let caller = Address::generate(&env);

    let result = admin::require_guardian(&env, &caller, &guardian);
    assert_eq!(result, Err(ContractError::NotGuardian));
}

#[test]
fn test_require_guardian_succeeds_for_correct_caller() {
    let env = Env::default();
    env.mock_all_auths();

    let guardian = Address::generate(&env);
    assert_eq!(admin::require_guardian(&env, &guardian, &guardian), Ok(()));
}

// ─── Error codes are distinct ─────────────────────────────────────────────────

#[test]
fn test_all_four_guard_failures_produce_distinct_error_codes() {
    // This test is the key assertion for issue #483: all four guards must
    // fail with different error codes so a caller can tell them apart.
    let env = Env::default();
    env.mock_all_auths();

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    let d = Address::generate(&env);
    let wrong = Address::generate(&env);

    let e1 = admin::require_admin(&env, &wrong, &a).unwrap_err();
    let e2 = admin::require_relay_or_admin(&env, &wrong, &b, &c).unwrap_err();
    let e3 = admin::require_pending_admin(&env, &wrong, &d).unwrap_err();
    let e4 = admin::require_guardian(&env, &wrong, &a).unwrap_err();

    // All four must be distinct.
    assert_ne!(e1, e2, "require_admin and require_relay_or_admin share an error code");
    assert_ne!(e1, e3, "require_admin and require_pending_admin share an error code");
    assert_ne!(e1, e4, "require_admin and require_guardian share an error code");
    assert_ne!(e2, e3, "require_relay_or_admin and require_pending_admin share an error code");
    assert_ne!(e2, e4, "require_relay_or_admin and require_guardian share an error code");
    assert_ne!(e3, e4, "require_pending_admin and require_guardian share an error code");

    // Each must be the documented variant.
    assert_eq!(e1, ContractError::NotAdmin);
    assert_eq!(e2, ContractError::NotRelayOrAdmin);
    assert_eq!(e3, ContractError::NotPendingAdmin);
    assert_eq!(e4, ContractError::NotGuardian);
}
