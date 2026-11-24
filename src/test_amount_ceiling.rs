//! Tests for the amount ceiling and the zero-amount guard (issue #472).
//!
//! The existing ceiling cases cover `validate_amount_ceiling`; the zero case
//! is added alongside, and both the transfer (`register_callback`) and
//! treasury-withdrawal paths are covered for the zero-amount rejection.

#![cfg(test)]

use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

use crate::types::ContractError;
use crate::validation::{validate_amount_ceiling, validate_nonzero_amount};
use crate::{SynapseCoreContract, SynapseCoreContractClient};

fn setup() -> (Env, SynapseCoreContractClient<'static>, Address, Address) {
    let env = Env::default();
    let contract_id = env.register(SynapseCoreContract, ());
    let client = SynapseCoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let relay = Address::generate(&env);
    let guardian = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &relay, &guardian);
    (env, client, admin, relay)
}

#[test]
fn amount_below_ceiling_is_ok() {
    assert_eq!(validate_amount_ceiling(50, 100), Ok(()));
}

#[test]
fn amount_at_ceiling_is_ok() {
    assert_eq!(validate_amount_ceiling(100, 100), Ok(()));
}

#[test]
fn amount_above_ceiling_is_err() {
    assert_eq!(
        validate_amount_ceiling(101, 100),
        Err(ContractError::AmountExceedsCeiling)
    );
}

#[test]
fn zero_amount_is_rejected() {
    assert_eq!(validate_nonzero_amount(0), Err(ContractError::ZeroAmount));
}

#[test]
fn negative_amount_is_rejected_as_zero_amount() {
    assert_eq!(validate_nonzero_amount(-5), Err(ContractError::ZeroAmount));
}

#[test]
fn positive_amount_is_ok() {
    assert_eq!(validate_nonzero_amount(1), Ok(()));
}

#[test]
fn register_callback_rejects_zero_amount() {
    let (_env, client, _admin, relay) = setup();
    let result = client.try_register_callback(&relay, &1u64, &0i128);
    assert_eq!(result, Err(Ok(ContractError::ZeroAmount)));
}

#[test]
fn register_callback_accepts_positive_amount() {
    let (_env, client, _admin, relay) = setup();
    assert_eq!(
        client.try_register_callback(&relay, &1u64, &100i128),
        Ok(Ok(()))
    );
}

#[test]
fn withdraw_treasury_rejects_zero_amount() {
    let (_env, client, admin, _relay) = setup();
    let result = client.try_withdraw_treasury(&admin, &0i128);
    assert_eq!(result, Err(Ok(ContractError::ZeroAmount)));
}
