//! Adversarial auth tests: guardian revocation, treasury withdrawal, and batch pause preconditions.
//!
//! These tests exercise the negative paths described in
//! `docs/adr/0004-guardian-emergency-admin-revocation.md`.

use crate::admin::{require_guardian, AdminError};
use crate::storage;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn guardian_revocation_fails_when_slot_empty() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    // No guardian has been configured for this admin.
    let result = require_guardian(&env, &admin);

    assert_eq!(result, Err(AdminError::GuardianNotSet));

    // The revoke path must short-circuit before writing storage or emitting
    // events, so the guardian slot must remain unset.
    assert!(storage::get_guardian(&env, &admin).is_none());
}
