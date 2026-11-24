//! Event emission helpers.
//!
//! `emit_tag_limit_exceeded` reports the supplied tag count and the limit when
//! a transaction exceeds `MAX_TAGS_PER_TRANSACTION` (issue #471). Soroban
//! errors are codes, not structured data, so the counts travel in the event
//! and the error carries only the stable code — letting a client render "you
//! sent N, limit is M".

use soroban_sdk::{symbol_short, Env, Symbol, Vec};

/// Emit the `taglimit` event carrying the supplied count and the limit.
pub fn emit_tag_limit_exceeded(env: &Env, supplied: u32, limit: u32) {
    let mut topics: Vec<Symbol> = Vec::new(env);
    topics.push_back(symbol_short!("taglimit"));
    let mut data: Vec<u32> = Vec::new(env);
    data.push_back(supplied);
    data.push_back(limit);
    env.events().publish(topics, data);
}
