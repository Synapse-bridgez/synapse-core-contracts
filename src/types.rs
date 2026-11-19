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
pub const DEFAULT_GLOBAL_MAX_AMOUNT: i128 = 1_000_000_000_000_i128;

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
    /// Off-chain processor has claimed the job; on-chain verification in progress.
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
    pub amount: i128,
    /// Asset code, e.g. "USDC", "USD" (max 12 chars per SEP-11).
    pub asset_code: String,
    /// Asset issuer address.
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
    UnbondRequest(Address),

    // ── Wave 2: Anchor Rebate (#145) ─────────────────────────────────────────
    /// Per-anchor tier config keyed by the anchor address.
    AnchorTier(Address),
}

/// Alias kept for modules that import `DataKey` by that name.
pub type DataKey = StorageKey;

// ─── Wave 2: Param Registry (#146) ────────────────────────────────────────────

/// Maximum length in bytes for a param name string.
///
/// Enforced by `set_param`; names longer than this are rejected with
/// [`ContractError::InvalidParamName`].
///
/// Chosen per commit 47dac6a's "#146 — Param Registry" design note:
/// "Max param name length: 32 bytes".
pub const MAX_PARAM_NAME_LEN: u32 = 32;

/// A single on-chain parameter entry.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::Param`].
/// All tunable values (fee rate, unbond delay, slash percentage, fee ceiling,
/// etc.) live here rather than as independent ad-hoc admin-settable fields.
///
/// ## Scaled-integer convention
///
/// Values are `i128` scaled integers. Rate-like params use **basis points**
/// (10 000 = 100 %). Count-like params use their natural unit.
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

// ─── Upgrade history (#86) ────────────────────────────────────────────────────

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
/// + two `u32` fields + one `Address`). At 32 entries that is ~3 200 bytes of
/// persistent rent — a flat, bounded cost regardless of how many upgrades have
/// been performed. Fewer entries would reduce audit trail usefulness; more
/// would linearly inflate the persistent-storage rent for every upgrade
/// forever. 32 entries provides years of audit history at any realistic
/// upgrade cadence while keeping the FIFO eviction budget flat.
pub const MAX_UPGRADE_HISTORY: u32 = 32;

/// A single on-chain record of a contract upgrade.
///
/// Stored as an element of the persistent upgrade-history vector, keyed by
/// [`StorageKey::UpgradeHistory`]. The vector is bounded at
/// [`MAX_UPGRADE_HISTORY`] entries; oldest entries are evicted first when the
/// cap is reached (ring-buffer / FIFO semantics).
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
    /// On-chain schema version at the time of this upgrade.
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
    /// Ledger sequence of the most recent top-up.
    pub updated_at_ledger: u32,
}

/// Pending unbond request from a relay signer.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::UnbondRequest`].
#[contracttype]
#[derive(Clone, Debug)]
pub struct UnbondRequest {
    /// Amount requested to unbond.
    pub amount: i128,
    /// Ledger sequence at which the unbond was requested.
    pub requested_at_ledger: u32,
    /// Ledger sequence at which the unbond may be claimed.
    pub claimable_at_ledger: u32,
}

// ─── Wave 2: Slash Evidence (#144) ────────────────────────────────────────────

/// On-chain-provable evidence of misbehaviour: two conflicting signed
/// callbacks for the same `transaction_id`.
///
/// Both payloads' `transaction_id` fields must equal `tx_id`, and at least
/// one field other than `idempotency_key` must differ.
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
///
/// Effective fee = `base_fee × (10_000 − rebate_bps) / 10_000`.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AnchorTierConfig {
    /// Anchor address this tier applies to.
    pub anchor: Address,
    /// Rebate in basis points (0–10 000).
    pub rebate_bps: u32,
    /// Human-readable label (max 16 bytes).
    pub label: String,
    /// Ledger sequence at which this tier was last set.
    pub updated_at_ledger: u32,
}

// ─── Treasury (#142) ──────────────────────────────────────────────────────────

/// Treasury configuration for the two-party withdrawal flow.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreasuryConfig {
    /// First authorized party (e.g. admin).
    pub party_a: Address,
    /// Second authorized party (e.g. treasury role).
    pub party_b: Address,
    /// Maximum amount that may be withdrawn per epoch.
    pub epoch_cap: i128,
    /// Length of an epoch in ledgers.
    pub epoch_length_ledgers: u32,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

/// All error codes returned by the contract.
#[contracterror]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum ContractError {
    // ── Initialisation ───────────────────────────────────────────────────────
    AlreadyInitialised = 1,
    NotInitialised = 2,

    // ── Authorisation ────────────────────────────────────────────────────────
    Unauthorised = 10,
    NotRelaySigner = 11,
    ContractPaused = 12,
    NoPendingAdminTransfer = 13,
    InvalidAdminNominee = 14,

    // ── Payload validation ───────────────────────────────────────────────────
    InvalidStellarAccount = 20,
    InvalidAmount = 21,
    InvalidAssetCode = 22,
    InvalidAssetIssuer = 23,
    MissingIdempotencyKey = 24,
    StringTooLong = 25,
    InvalidPayload = 26,

    // ── Transaction lifecycle ────────────────────────────────────────────────
    TransactionNotFound = 30,
    InvalidStatusTransition = 31,

    // ── Idempotency ──────────────────────────────────────────────────────────
    DuplicateRequest = 40,

    // ── Storage ──────────────────────────────────────────────────────────────
    StorageError = 50,

    // ── Upgrade safety ───────────────────────────────────────────────────────
    SchemaVersionMismatch = 60,
    UnknownMigration = 61,
    MigrationFailed = 62,
    UpgradeNotReversible = 63,
    /// `post_upgrade_self_check` detected missing or inconsistent core state.
    SelfCheckFailed = 64,

    // ── Wave 2: Param Registry (#146) ────────────────────────────────────────
    InvalidParamName = 70,
    InvalidParamValue = 71,
    ParamNotFound = 72,

    // ── Wave 2: Collateral Bonding (#143) ────────────────────────────────────
    InvalidBondAmount = 80,
    InsufficientBond = 81,
    UnbondAlreadyPending = 82,
    UnbondDelayNotElapsed = 83,
    NoPendingUnbond = 84,
    TimelockNotElapsed = 85,

    // ── Wave 2: Slashing (#144) ──────────────────────────────────────────────
    EvidenceTxIdMismatch = 90,
    EvidenceNotConflicting = 91,
    SignerNotBonded = 92,
    InvalidSlashEvidence = 93,

    // ── Wave 2: Anchor Rebate (#145) ─────────────────────────────────────────
    InvalidRebateBps = 100,
    InvalidTierLabel = 101,
    AnchorTierNotFound = 102,

    // ── Amount ceiling (#169) ────────────────────────────────────────────────
    AmountCeilingExceeded = 110,

    // ── Treasury (#142) ──────────────────────────────────────────────────────
    TreasuryNotConfigured = 120,
    EpochCapExceeded = 121,
    InsufficientTreasuryBalance = 122,
    SamePartyWithdrawal = 123,
    WithdrawalNotFound = 124,

    // ── Relay signer set ─────────────────────────────────────────────────────
    SignerAlreadyExists = 130,

    // ── Batch operations ─────────────────────────────────────────────────────
    BatchTooLarge = 140,
    BatchWriteBudgetExceeded = 141,
    BatchEventBudgetExceeded = 142,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{Env, String};

    // ── ParamEntry round-trip (#146) ──────────────────────────────────────────

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

    #[test]
    fn param_name_max_len_is_32() {
        assert_eq!(MAX_PARAM_NAME_LEN, 32);
    }

    #[test]
    fn storage_key_param_does_not_alias_other_variants() {
        let env = Env::default();
        let k1 = StorageKey::Param(String::from_str(&env, "slash_bps"));
        let k2 = StorageKey::Param(String::from_str(&env, "base_fee_bps"));
        let admin = soroban_sdk::Address::generate(&env);
        let e1 = ParamEntry {
            value: 1,
            updated_at_ledger: 0,
            updated_by: admin.clone(),
        };
        let e2 = ParamEntry {
            value: 2,
            updated_at_ledger: 0,
            updated_by: admin.clone(),
        };
        env.storage().persistent().set(&k1, &e1);
        env.storage().persistent().set(&k2, &e2);
        let r1: ParamEntry = env.storage().persistent().get(&k1).unwrap();
        let r2: ParamEntry = env.storage().persistent().get(&k2).unwrap();
        assert_eq!(r1.value, 1);
        assert_eq!(r2.value, 2);
    }

    // ── UpgradeRecord round-trip (#86) ────────────────────────────────────────

    /// Verifies `UpgradeRecord` survives a storage round-trip via
    /// `StorageKey::UpgradeHistory`, confirming `#[contracttype]` is in place
    /// and all fields serialise correctly.
    #[test]
    fn upgrade_record_round_trips() {
        let env = Env::default();
        let admin = soroban_sdk::Address::generate(&env);
        let prev = soroban_sdk::BytesN::from_array(&env, &[0xAAu8; 32]);
        let new = soroban_sdk::BytesN::from_array(&env, &[0xBBu8; 32]);

        let record = UpgradeRecord {
            previous_wasm_hash: prev.clone(),
            new_wasm_hash: new.clone(),
            schema_version: 1,
            ledger: 500,
            admin: admin.clone(),
        };

        // Store as a Vec (the real storage shape) to prove the type works
        // inside a soroban_sdk::Vec, which is what append_upgrade_record uses.
        let key = StorageKey::UpgradeHistory;
        let mut vec: soroban_sdk::Vec<UpgradeRecord> = soroban_sdk::Vec::new(&env);
        vec.push_back(record.clone());
        env.storage().persistent().set(&key, &vec);

        let loaded: soroban_sdk::Vec<UpgradeRecord> = env.storage().persistent().get(&key).unwrap();
        assert_eq!(loaded.len(), 1);
        let r = loaded.get_unchecked(0);
        assert_eq!(r.previous_wasm_hash, prev);
        assert_eq!(r.new_wasm_hash, new);
        assert_eq!(r.schema_version, 1);
        assert_eq!(r.ledger, 500);
        assert_eq!(r.admin, admin);
    }

    /// Confirms `MAX_UPGRADE_HISTORY` equals 32 as documented in
    /// COST_MODEL.md §10.
    #[test]
    fn max_upgrade_history_is_32() {
        assert_eq!(MAX_UPGRADE_HISTORY, 32);
    }

    /// Confirms `StorageKey::UpgradeHistory` and `StorageKey::CurrentWasmHash`
    /// are usable as independent storage keys without aliasing each other.
    #[test]
    fn upgrade_storage_keys_do_not_alias() {
        let env = Env::default();
        let hash_a = soroban_sdk::BytesN::from_array(&env, &[0x01u8; 32]);
        let hash_b = soroban_sdk::BytesN::from_array(&env, &[0x02u8; 32]);

        env.storage()
            .persistent()
            .set(&StorageKey::CurrentWasmHash, &hash_a);

        // UpgradeHistory holds a Vec — store an empty one so the key exists.
        let empty: soroban_sdk::Vec<UpgradeRecord> = soroban_sdk::Vec::new(&env);
        env.storage()
            .persistent()
            .set(&StorageKey::UpgradeHistory, &empty);

        let loaded_hash: soroban_sdk::BytesN<32> = env
            .storage()
            .persistent()
            .get(&StorageKey::CurrentWasmHash)
            .unwrap();
        assert_eq!(loaded_hash, hash_a);
        assert_ne!(loaded_hash, hash_b);

        let loaded_history: soroban_sdk::Vec<UpgradeRecord> = env
            .storage()
            .persistent()
            .get(&StorageKey::UpgradeHistory)
            .unwrap();
        assert_eq!(loaded_history.len(), 0);
    }
}
