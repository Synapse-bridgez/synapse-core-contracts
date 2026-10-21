use soroban_sdk::{Address, Env};

use crate::types::{BondRecord, UnbondRequest};

/// Storage key for a signer's bond record.
#[soroban_sdk::contracttype]
#[derive(Clone)]
pub enum BondKey {
    Bond(Address),
    Unbond(Address),
}

/// Read the bond record for `signer`, returning a zero/default record when the
/// signer has never bonded.
pub fn get_bond_record(env: &Env, signer: &Address) -> BondRecord {
    env.storage()
        .persistent()
        .get(&BondKey::Bond(signer.clone()))
        .unwrap_or(BondRecord {
            signer: signer.clone(),
            amount: 0,
            bonded_at: 0,
        })
}

/// Read the pending unbond request for `signer`, or `None` when no request is
/// pending (including after a completed unbond has been claimed).
pub fn get_unbond_request(env: &Env, signer: &Address) -> Option<UnbondRequest> {
    env.storage()
        .persistent()
        .get(&BondKey::Unbond(signer.clone()))
}
