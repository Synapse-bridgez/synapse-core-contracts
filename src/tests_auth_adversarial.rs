//! Adversarial authorization tests for the admin transfer flow.

use soroban_sdk::testutils::Address as _;
use soroban_sdk::Address;

use crate::admin;
use crate::errors::ContractError;
use crate::storage;
use crate::test_support;

#[test]
fn propose_admin_rejects_current_admin() {
    let env = test_support::setup();
    let current_admin = storage::get_admin(&env);

    let result = admin::require_new_admin(&env, &current_admin);
    assert_eq!(result, Err(ContractError::Unauthorized));

    // The pending slot must be untouched by the rejected proposal.
    assert!(storage::get_pending_admin(&env).is_none());
}

#[test]
fn accept_admin_rejects_current_admin_with_stale_pending() {
    let env = test_support::setup();
    let current_admin = storage::get_admin(&env);

    // Simulate a stale pending value that points at the current admin.
    storage::set_pending_admin(&env, &current_admin);

    let result = admin::require_pending_admin(&env, &current_admin);
    assert_eq!(result, Err(ContractError::Unauthorized));

    // The stale pending slot must remain unchanged after the rejection.
    assert_eq!(storage::get_pending_admin(&env), Some(current_admin));
}

#[test]
fn accept_admin_rejects_non_pending_caller() {
    let env = test_support::setup();
    let stranger = Address::generate(&env);

    let result = admin::require_pending_admin(&env, &stranger);
    assert_eq!(result, Err(ContractError::Unauthorized));
}
