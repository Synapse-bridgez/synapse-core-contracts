//! # Types
//!
//! On-chain equivalents of the `synapse-core` Rust service's domain model.
//! Every struct that touches ledger storage derives [`soroban_sdk::contracttype`].
//!
//! ## Scaled-integer convention
//!
//! All rate-like values in this module (fee rates, rebate percentages, slash
//! percentages) are stored as **basis points** (`i128`), where
//! `10_000 bps = 100 %`. Count-like values (e.g. `unbond_delay_ledgers`) are
//! stored as plain integers in their natural unit (ledgers). The convention is
//! documented once here rather than re-derived at every call site:
//!
//! | Param name             | Unit           | Example                      |
//! |------------------------|----------------|------------------------------|
//! | `base_fee_bps`         | basis points   | `200` = 2 %                  |
//! | `slash_bps`            | basis points   | `10_000` = 100 %             |
//! | `unbond_delay_ledgers` | ledger count   | `17_280` ≈ 24 h @ 5 s/ledger |

use soroban_sdk::{contracterror, contracttype, Address, String};

/// Current on-chain storage schema version.
///
/// Bump this whenever [`Transaction`] or [`StorageKey`] layout changes in a
/// way that a running upgrade needs to be aware of.
pub const SCHEMA_VERSION: u32 = 1;

/// Default global maximum amount for a single transaction (in stroops).
///
/// Applied when the `global_max_amount` param has not been explicitly set.
/// Equivalent to 1 000 000 XLM — deliberately conservative.
pub const DEFAULT_GLOBAL_MAX_AMOUNT: i128 = 1_000_000_000_000_i128; // 1 M XLM in stroops

/// Maximum number of transactions allowed in a single `batch_register_callback`
/// call.
pub const MAX_BATCH_SIZE: u32 = 100;

// ─── Transaction status ───────────────────────────────────────────────────────

/// Mirrors the `status` column in the `transactions` table.
///
/// State machine:
/// ```text
/// Pending ──► Processing ──► Completed
///         └───────────────► Failed
///         └───────────────► Cancelled
/// ```
#[contracttype]
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TransactionStatus {
    /// Initial state — callback received, not yet picked up by the processor.
    Pending,
    /// Off-chain processor has claimed the job; on-chain verification in
    /// progress.
    Processing,
    /// Stellar on-chain settlement confirmed.
    Completed,
    /// Terminal failure — reason stored in [`Transaction::failure_reason`].
    Failed,
    /// Operator-initiated cancellation.
    Cancelled,
    /// Transaction is being retried after a transient failure.
    Retrying,
}

// ─── Callback type ────────────────────────────────────────────────────────────

/// Maps the `callback_type` field from the Anchor Platform webhook.
#[contracttype]
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CallbackType {
    Deposit,
    Withdrawal,
}

// ─── Core transaction record ──────────────────────────────────────────────────

/// On-chain mirror of the `transactions` table row.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::Transaction`].
#[contracttype]
#[derive(Clone, Debug)]
pub struct Transaction {
    /// UUID-style unique identifier (generated off-chain, echoed here).
    pub id: String,

    /// Stellar account address of the depositor (G… address, 56 chars).
    pub stellar_account: String,

    /// Deposit amount in stroops (1 XLM = 10 000 000 stroops).
    /// Stored as i128 to match Soroban's native token amount convention.
    pub amount: i128,

    /// Asset code, e.g. "USDC", "USD" (max 12 chars per SEP-11).
    pub asset_code: String,

    /// Asset issuer address. Combined with `asset_code` uniquely identifies
    /// the Stellar asset.
    pub asset_issuer: String,

    /// Current lifecycle status.
    pub status: TransactionStatus,

    /// Ledger sequence number when the transaction was first registered.
    pub created_at_ledger: u32,

    /// Ledger sequence number of the last status update.
    pub updated_at_ledger: u32,

    /// Opaque ID from the Anchor Platform callback payload.
    pub anchor_transaction_id: String,

    /// `deposit` or `withdrawal` — mirrors `callback_type`.
    pub callback_type: CallbackType,

    /// Raw status string received from the Anchor Platform.
    pub callback_status: String,

    /// Stellar transaction hash recorded after on-chain settlement.
    /// Empty string until the transaction reaches `Completed`.
    pub stellar_tx_hash: String,

    /// Short failure reason code — populated only on `Failed`.
    pub failure_reason: String,
}

// ─── Incoming webhook payload ─────────────────────────────────────────────────

/// Payload forwarded by the trusted relay signer when calling
/// [`crate::SynapseCoreContract::register_callback`].
#[contracttype]
#[derive(Clone, Debug)]
pub struct CallbackPayload {
    /// Must match an existing or newly-generated transaction UUID.
    pub transaction_id: String,

    /// Stellar account that initiated the deposit.
    pub stellar_account: String,

    /// Deposit amount in stroops.
    pub amount: i128,

    /// Asset code.
    pub asset_code: String,

    /// Asset issuer address.
    pub asset_issuer: String,

    /// Matches `X-Idempotency-Key` header from the Anchor Platform webhook.
    pub idempotency_key: String,

    /// Anchor Platform's internal transaction ID.
    pub anchor_transaction_id: String,

    /// Callback type from the Anchor Platform.
    pub callback_type: CallbackType,

    /// Raw status from the Anchor Platform callback.
    pub callback_status: String,
}

// ─── Storage keys ─────────────────────────────────────────────────────────────

/// Discriminants used as ledger storage keys.
///
/// Persistent storage keys (admin, relay signer, init flag) use
/// `Symbol`-based variants. Per-transaction data is keyed by the transaction
/// ID string.
#[contracttype]
#[derive(Clone, Debug)]
pub enum StorageKey {
    /// Singleton: whether `initialize()` has been called.
    Initialised,
    /// Singleton: current admin address.
    Admin,
    /// Singleton: trusted relay signer address.
    RelaySigner,
    /// Singleton: N-of-M relay signer set.
    RelaySigners,
    /// Singleton: N-of-M relay approval threshold.
    RelayThreshold,
    /// Singleton: emergency-pause flag.
    Paused,
    /// Per-transaction record keyed by transaction ID string.
    Transaction(String),
    /// Per-transaction record keyed by numeric transaction ID.
    TransactionById(u64),
    /// Tags attached to a transaction, keyed by numeric transaction ID.
    TransactionTags(u64),
    /// Idempotency key → cached response ledger.
    IdempotencyKey(String),
    /// Singleton: address nominated to become admin, pending `accept_admin()`.
    PendingAdmin,
    /// Singleton: on-chain storage schema version.
    SchemaVersion,
    /// Singleton: accrued treasury fee total.
    TreasuryAccrued,
    /// Relay approvals for a given call id (temporary storage).
    RelayApprovals(u64),
    /// Arbitrary relay/admin flag.
    Relayer(Address),
    /// Slot-based status index for `get_transactions_by_status`.
    StatusIndex(TransactionStatus),
    /// Snapshot taken immediately before a WASM upgrade.
    UpgradeSnapshot,
    /// Per-anchor amount ceiling.
    AnchorAmountCeiling(String),
    /// Singleton: global maximum transaction amount.
    GlobalMaxAmount,
    /// Singleton: pre-upgrade state snapshot key.
    UpgradeSnapshotKey,
    /// Merged-into mapping: source tx_id → canonical tx_id.
    MergedInto(String),

    // ── Upgrade history (#86 / issue #27 / issue #100) ───────────────────────
    /// Singleton: bounded ring-buffer of [`UpgradeRecord`] entries.
    /// Capped at [`MAX_UPGRADE_HISTORY`] entries; oldest evicted on overflow.
    UpgradeHistory,
    /// Singleton: WASM hash currently installed on this contract.
    /// Written on every successful upgrade; read by `post_upgrade_self_check`.
    CurrentWasmHash,

    // ── Wave 2: Param Registry (#146) ───────────────────────────────────────
    /// Per-parameter record keyed by param name string.
    ///
    /// Well-known names: `unbond_delay_ledgers`, `slash_bps`, `base_fee_bps`.
    Param(String),

    // ── Wave 2: Collateral Bonding (#143) ───────────────────────────────────
    /// Per-signer bond record keyed by the signer address.
    BondRecord(Address),
    /// Per-signer unbond request keyed by the signer address.
    /// Absent when no unbond is pending.
    UnbondRequest(Address),

    // ── Wave 2: Anchor Rebate (#145) ─────────────────────────────────────────
    /// Per-anchor tier config keyed by the anchor address.
    AnchorTier(Address),
}

/// Alias kept for modules that import `DataKey` by that name.
pub type DataKey = StorageKey;

// ─── Wave 2: Param Registry (#146) ────────────────────────────────────────────

/// A single on-chain parameter entry.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::Param`].
/// All tunable values (fee rate, unbond delay, slash percentage, fee ceiling,
/// etc.) live here rather than as independent ad-hoc admin-settable fields.
///
/// ## Scaled-integer convention
///
/// Values are `i128` scaled integers. Rate-like params (fee rates, percentages)
/// use **basis points** (10 000 = 100 %). Count-like params (delays, limits)
/// use their natural unit. The semantics of each well-known key are:
///
/// | Key                    | Semantics                                         |
/// |------------------------|---------------------------------------------------|
/// | `unbond_delay_ledgers` | Ledgers a signer must wait before claiming unbond |
/// | `slash_bps`            | Slash percentage in basis points (10 000 = 100 %) |
/// | `base_fee_bps`         | Protocol base fee in basis points                 |
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ParamEntry {
    /// Param value. Represented as `i128` to accommodate both integer counts
    /// and scaled basis-point rates (e.g. 9 500 = 95.00 %).
    pub value: i128,
    /// Ledger sequence at which this param was last updated.
    pub updated_at_ledger: u32,
    /// Address that last set this param (always the admin).
    pub updated_by: Address,
}

// ─── Wave 2: Collateral Bonding (#143) ────────────────────────────────────────

/// Collateral bond record for a relay signer.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::BondRecord`].
#[contracttype]
#[derive(Clone, Debug)]
pub struct BondRecord {
    /// The signer whose collateral is bonded.
    pub signer: Address,
    /// Amount currently bonded (in stroops / token's smallest unit).
    pub amount: i128,
    /// Ledger sequence at which the current bond was first created.
    pub bonded_at_ledger: u32,
    /// Ledger sequence of the most recent top-up (same as `bonded_at_ledger`
    /// if no top-up has occurred since the initial bond).
    pub updated_at_ledger: u32,
}

/// Pending unbond request from a relay signer.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::UnbondRequest`].
/// The existence of this record is proof that an unbond has been requested
/// but has not yet been claimed (the timelock has not elapsed).
#[contracttype]
#[derive(Clone, Debug)]
pub struct UnbondRequest {
    /// Amount requested to unbond.
    pub amount: i128,
    /// Ledger sequence at which the unbond was requested.
    pub requested_at_ledger: u32,
    /// Ledger sequence at which the unbond may be claimed
    /// (= `requested_at_ledger` + `unbond_delay_ledgers` param at request
    /// time).
    pub claimable_at_ledger: u32,
}

// ─── Wave 2: Slash Evidence (#144) ────────────────────────────────────────────

/// On-chain-provable evidence of misbehaviour: two conflicting signed
/// callbacks for the same `transaction_id`.
///
/// ## Validity rules (enforced by `slash_signer`)
///
/// * Both payloads' `transaction_id` fields must equal `tx_id`.
/// * At least one field *other than* `idempotency_key` must differ between
///   `payload_a` and `payload_b` — a difference in `idempotency_key` alone
///   is explicitly **not** conflicting.
#[contracttype]
#[derive(Clone, Debug)]
pub struct SlashEvidence {
    /// The `transaction_id` that appears in both conflicting callbacks.
    pub tx_id: String,
    /// Payload of callback A.
    pub payload_a: CallbackPayload,
    /// Payload of callback B (the conflicting second callback).
    pub payload_b: CallbackPayload,
}

// ─── Wave 2: Anchor Tier / Rebate (#145) ──────────────────────────────────────

/// Trust/volume tier config for a single anchor address.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::AnchorTier`].
/// The admin sets this to indicate that a particular anchor qualifies for a
/// reduced effective fee rate.
///
/// Effective fee = `base_fee × (10_000 − rebate_bps) / 10_000`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AnchorTierConfig {
    /// Anchor address this tier applies to.
    pub anchor: Address,
    /// Rebate in basis points (0–10 000).
    ///
    /// `0`      = no rebate (full fee rate applies).  
    /// `500`    = 5 % rebate.  
    /// `10_000` = 100 % rebate (zero effective fee).
    pub rebate_bps: u32,
    /// Human-readable label for the tier (e.g. "gold", "silver", "standard").
    /// Max 16 bytes; purely informational.
    pub label: String,
    /// Ledger sequence at which this tier was last set.
    pub updated_at_ledger: u32,
}

// ─── Treasury (#142) ──────────────────────────────────────────────────────────

/// Treasury configuration for the two-party withdrawal flow.
///
/// A withdrawal requires one authorized party to propose and a *distinct*
/// second authorized party to authorize before funds move (accepted risk R-06,
/// `THREAT_MODEL.md` §8).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreasuryConfig {
    /// First authorized party (e.g. admin).
    pub party_a: Address,
    /// Second authorized party (e.g. treasury role).
    pub party_b: Address,
    /// Maximum amount that may be withdrawn per epoch.
    pub epoch_cap: i128,
    /// Length of an epoch in ledgers; resets `epoch_withdrawn` counter.
    pub epoch_length_ledgers: u32,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

/// All error codes returned by the contract.
///
/// Uses [`contracterror`] so they surface correctly via the Soroban XDR and
/// can be decoded by SDK clients / frontends.
#[contracterror]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum ContractError {
    // ── Initialisation ───────────────────────────────────────────────────────
    /// `initialize()` has already been called.
    AlreadyInitialised = 1,
    /// Contract has not yet been initialised.
    NotInitialised = 2,

    // ── Authorisation ────────────────────────────────────────────────────────
    /// Caller is not the admin.
    Unauthorised = 10,
    /// Caller is not the trusted relay signer.
    NotRelaySigner = 11,
    /// The contract is paused; the requested operation is temporarily disabled.
    ContractPaused = 12,
    /// `accept_admin` was called with no pending admin transfer in progress.
    NoPendingAdminTransfer = 13,
    /// `propose_admin` was called with the contract's own address as nominee.
    InvalidAdminNominee = 14,

    // ── Payload validation ───────────────────────────────────────────────────
    /// `stellar_account` field is malformed.
    InvalidStellarAccount = 20,
    /// `amount` is zero or negative.
    InvalidAmount = 21,
    /// `asset_code` is empty or exceeds 12 characters.
    InvalidAssetCode = 22,
    /// `asset_issuer` is malformed.
    InvalidAssetIssuer = 23,
    /// `idempotency_key` is empty.
    MissingIdempotencyKey = 24,
    /// A `String` field exceeds its maximum allowed length.
    StringTooLong = 25,
    /// Generic payload validation failure (malformed, empty required field, etc.)
    InvalidPayload = 26,

    // ── Transaction lifecycle ────────────────────────────────────────────────
    /// No transaction with the given ID exists in storage.
    TransactionNotFound = 30,
    /// The requested status transition violates the state machine.
    InvalidStatusTransition = 31,

    // ── Idempotency ──────────────────────────────────────────────────────────
    /// Request is a duplicate within the retention window.
    DuplicateRequest = 40,

    // ── Storage ──────────────────────────────────────────────────────────────
    /// A ledger read/write produced an unexpected result.
    StorageError = 50,

    // ── Upgrade safety ───────────────────────────────────────────────────────
    /// `upgrade()`'s `expected_schema_version` did not match the on-chain
    /// value; the upgrade was aborted.
    SchemaVersionMismatch = 60,
    /// The requested migration id is not registered in this WASM.
    UnknownMigration = 61,
    /// The migration exceeded the allowed storage-touch budget.
    MigrationFailed = 62,
    /// A rollback was attempted for an upgrade that is not reversible.
    UpgradeNotReversible = 63,

    // ── Wave 2: Param Registry (#146) ────────────────────────────────────────
    /// `set_param` was called with an empty or too-long param name (> 32 bytes).
    InvalidParamName = 70,
    /// `set_param` value is outside the allowed range for that parameter.
    InvalidParamValue = 71,
    /// `get_param` was called for a param that has never been set.
    ParamNotFound = 72,

    // ── Wave 2: Collateral Bonding (#143) ────────────────────────────────────
    /// `bond_collateral` amount is zero or negative.
    InvalidBondAmount = 80,
    /// `unbond_collateral` requested more than the currently bonded balance.
    InsufficientBond = 81,
    /// `unbond_collateral` was called while a previous unbond request is still
    /// pending.
    UnbondAlreadyPending = 82,
    /// `claim_unbond` was called before the unbond delay has elapsed.
    UnbondDelayNotElapsed = 83,
    /// `claim_unbond` was called but no pending unbond request exists.
    NoPendingUnbond = 84,
    /// `claim_unbond` / timelock guard: the required delay has not elapsed.
    TimelockNotElapsed = 85,

    // ── Wave 2: Slashing (#144) ──────────────────────────────────────────────
    /// `slash_signer` evidence `tx_id` does not match both payloads'
    /// `transaction_id` fields.
    EvidenceTxIdMismatch = 90,
    /// `slash_signer` evidence payloads are identical (no conflicting content).
    EvidenceNotConflicting = 91,
    /// `slash_signer` was called for a signer with no bonded collateral.
    SignerNotBonded = 92,
    /// `slash_signer` evidence is otherwise invalid.
    InvalidSlashEvidence = 93,

    // ── Wave 2: Anchor Rebate (#145) ─────────────────────────────────────────
    /// `set_anchor_tier` rebate_bps exceeds 10 000 (100 %).
    InvalidRebateBps = 100,
    /// `set_anchor_tier` label exceeds the maximum allowed length (16 bytes).
    InvalidTierLabel = 101,
    /// `get_anchor_tier` / `compute_effective_fee` anchor has no tier set.
    AnchorTierNotFound = 102,

    // ── Amount ceiling (#169) ────────────────────────────────────────────────
    /// Transaction amount exceeds the configured ceiling (global or per-anchor).
    AmountCeilingExceeded = 110,

    // ── Treasury (#142) ──────────────────────────────────────────────────────
    /// Treasury config has not been set.
    TreasuryNotConfigured = 120,
    /// Withdrawal amount exceeds the per-epoch cap.
    EpochCapExceeded = 121,
    /// Withdrawal amount exceeds the available treasury balance.
    InsufficientTreasuryBalance = 122,
    /// A party attempted to both propose and authorize the same withdrawal.
    SamePartyWithdrawal = 123,
    /// The withdrawal id does not exist.
    WithdrawalNotFound = 124,

    // ── Relay signer set ─────────────────────────────────────────────────────
    /// The signer is already in the relay-signer set.
    SignerAlreadyExists = 130,

    // ── Batch operations ─────────────────────────────────────────────────────
    /// `batch_register_callback` count exceeds `MAX_BATCH_SIZE`.
    BatchTooLarge = 140,
    /// `batch_register_callback` write-budget estimate exceeded.
    BatchWriteBudgetExceeded = 141,
    /// `batch_register_callback` event-budget estimate exceeded.
    BatchEventBudgetExceeded = 142,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{Env, String};

    // ── ParamEntry round-trip (#146) ──────────────────────────────────────────

    /// Verifies `ParamEntry` survives a storage round-trip for the three
    /// well-known keys at boundary values, as required by issue #146's
    /// acceptance criteria.
    ///
    /// Well-known keys tested:
    /// * `unbond_delay_ledgers` — ledger count; tested at 0, max i128, and a
    ///   realistic value (17 280 ≈ 24 h).
    /// * `slash_bps` — basis points; tested at 0, 10 000 (100 %), and negative
    ///   (negative slash is invalid in practice but the type must round-trip).
    /// * `base_fee_bps` — basis points; tested at 0, 10 000, and i128::MAX.
    #[test]
    fn param_entry_round_trips_unbond_delay_ledgers() {
        let env = Env::default();
        let admin = soroban_sdk::Address::generate(&env);

        for value in [0_i128, 17_280, i128::MAX] {
            let entry = ParamEntry {
                value,
                updated_at_ledger: 42,
                updated_by: admin.clone(),
            };
            // StorageKey::Param must be usable as a persistent storage key.
            let key = StorageKey::Param(String::from_str(&env, "unbond_delay_ledgers"));
            env.storage().persistent().set(&key, &entry);
            let loaded: ParamEntry = env.storage().persistent().get(&key).unwrap();
            assert_eq!(loaded, entry, "round-trip failed for value={value}");
        }
    }

    #[test]
    fn param_entry_round_trips_slash_bps() {
        let env = Env::default();
        let admin = soroban_sdk::Address::generate(&env);

        for value in [0_i128, 5_000, 10_000, -1] {
            let entry = ParamEntry {
                value,
                updated_at_ledger: 1,
                updated_by: admin.clone(),
            };
            let key = StorageKey::Param(String::from_str(&env, "slash_bps"));
            env.storage().persistent().set(&key, &entry);
            let loaded: ParamEntry = env.storage().persistent().get(&key).unwrap();
            assert_eq!(loaded, entry, "round-trip failed for value={value}");
        }
    }

    #[test]
    fn param_entry_round_trips_base_fee_bps() {
        let env = Env::default();
        let admin = soroban_sdk::Address::generate(&env);

        for value in [0_i128, 200, 10_000, i128::MAX] {
            let entry = ParamEntry {
                value,
                updated_at_ledger: 999,
                updated_by: admin.clone(),
            };
            let key = StorageKey::Param(String::from_str(&env, "base_fee_bps"));
            env.storage().persistent().set(&key, &entry);
            let loaded: ParamEntry = env.storage().persistent().get(&key).unwrap();
            assert_eq!(loaded, entry, "round-trip failed for value={value}");
        }
    }

    // ── Name-length validation constants ─────────────────────────────────────

    /// Confirms the documented 32-byte name cap is what the constant says it
    /// is, so callers linking against this crate can rely on it without
    /// reading prose.
    #[test]
    fn param_name_max_len_is_32() {
        assert_eq!(MAX_PARAM_NAME_LEN, 32);
    }

    // ── StorageKey::Param discriminant is distinct ────────────────────────────

    #[test]
    fn storage_key_param_does_not_alias_other_variants() {
        let env = Env::default();
        let k1 = StorageKey::Param(String::from_str(&env, "slash_bps"));
        let k2 = StorageKey::Param(String::from_str(&env, "base_fee_bps"));
        let admin = soroban_sdk::Address::generate(&env);
        let e1 = ParamEntry { value: 1, updated_at_ledger: 0, updated_by: admin.clone() };
        let e2 = ParamEntry { value: 2, updated_at_ledger: 0, updated_by: admin.clone() };
        env.storage().persistent().set(&k1, &e1);
        env.storage().persistent().set(&k2, &e2);
        let r1: ParamEntry = env.storage().persistent().get(&k1).unwrap();
        let r2: ParamEntry = env.storage().persistent().get(&k2).unwrap();
        assert_eq!(r1.value, 1);
        assert_eq!(r2.value, 2);
    }
}

/// Maximum length in bytes for a param name string.
///
/// Enforced by `set_param`; names longer than this are rejected with
/// [`ContractError::InvalidParamName`].
///
/// Chosen per commit 47dac6a's "#146 — Param Registry" design note:
/// "Max param name length: 32 bytes".
pub const MAX_PARAM_NAME_LEN: u32 = 32;

// ─── Upgrade history (#86 / issue #27 / issue #100) ───────────────────────────

/// Maximum number of [`UpgradeRecord`] entries retained in the on-chain
/// upgrade history.
///
/// When this cap is reached the oldest entry is evicted before the new one is
/// appended (ring-buffer / FIFO semantics). The eviction logic itself lives in
/// `storage::StorageClient::append_upgrade_record` (issue #27); this constant
/// is defined here so both the storage layer and callers share a single source
/// of truth.
///
/// ## Value rationale (COST_MODEL.md §10)
///
/// Per COST_MODEL.md §10, `append_upgrade_record` performs 1 read + 1 write
/// of the history vector on every upgrade. At Protocol 22 rates a persistent
/// vector entry costs approximately 1 stroop per byte per 100 000 ledgers in
/// rent. Each [`UpgradeRecord`] is approximately 100 bytes (two 32-byte hashes
/// + two u32 fields + one Address). At 32 entries that is ~3 200 bytes of
/// persistent rent — a flat, bounded cost regardless of how many upgrades have
/// been performed. Fewer entries would reduce audit trail usefulness; more
/// would linearly inflate the persistent-storage rent for every upgrade
/// forever. 32 entries provides ~years of audit history at any realistic
/// upgrade cadence while keeping the FIFO eviction budget flat.
pub const MAX_UPGRADE_HISTORY: u32 = 32;

/// A single on-chain record of a contract upgrade.
///
/// Stored as an element of the persistent upgrade-history vector, keyed by
/// [`StorageKey::UpgradeHistory`]. The vector is bounded at
/// [`MAX_UPGRADE_HISTORY`] entries; oldest entries are evicted first when the
/// cap is reached.
///
/// The ring-buffer append logic lives in
/// `storage::StorageClient::append_upgrade_record` (issue #27).
/// The `get_upgrade_history` query entry point is issue #27.
/// The entry points that populate this record on a successful upgrade are
/// issue #100.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeRecord {
    /// WASM hash that was live immediately **before** this upgrade.
    pub previous_wasm_hash: soroban_sdk::BytesN<32>,
    /// WASM hash installed by this upgrade.
    pub new_wasm_hash: soroban_sdk::BytesN<32>,
    /// On-chain schema version at the time of this upgrade (the value
    /// recorded by `initialize` / the previous upgrade).
    pub schema_version: u32,
    /// Ledger sequence number at which the upgrade was executed.
    pub ledger: u32,
    /// Admin address that authorised this upgrade.
    pub admin: Address,
}

/// Verdict returned by the `simulate_upgrade` dry-run entry point (issue #85).
///
/// Each variant maps to exactly one guard in the real `upgrade()` flow so
/// callers can distinguish auth failures, schema mismatches, and clean
/// compatibility without committing any state.
///
/// Out of scope for this issue — defined here because `test_pause.rs`
/// references `crate::types::UpgradeCompatibility` in its quarantined
/// simulate-upgrade tests, and the type must at least parse to avoid breaking
/// the gated-test index in QUARANTINE.md.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpgradeCompatibility {
    /// All guards pass; the upgrade may proceed.
    Compatible,
    /// The `expected_schema_version` argument did not match the on-chain value.
    SchemaVersionMismatch,
    /// The caller is not the current admin.
    CallerNotAdmin,
    /// The contract has not been initialised yet.
    NotInitialised,
}
