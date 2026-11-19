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
//! stored as plain integers in their natural unit (ledgers).
//!
//! | Param name             | Unit           | Example                      |
//! |------------------------|----------------|------------------------------|
//! | `base_fee_bps`         | basis points   | `200` = 2 %                  |
//! | `slash_bps`            | basis points   | `10_000` = 100 %             |
//! | `unbond_delay_ledgers` | ledger count   | `17_280` ≈ 24 h @ 5 s/ledger |

use soroban_sdk::{contracterror, contracttype, Address, String};

pub const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_GLOBAL_MAX_AMOUNT: i128 = 1_000_000_000_000_i128;
pub const MAX_BATCH_SIZE: u32 = 100;
pub const MAX_PARAM_NAME_LEN: u32 = 32;
pub const MAX_UPGRADE_HISTORY: u32 = 32;
pub const MAX_OPEN_DISPUTES: u32 = 100;

// ─── Transaction status ───────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TransactionStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Cancelled,
    Retrying,
}

// ─── Callback type ────────────────────────────────────────────────────────────

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
    pub id: String,
    pub stellar_account: String,
    pub amount: i128,
    pub asset_code: String,
    pub asset_issuer: String,
    pub status: TransactionStatus,
    pub created_at_ledger: u32,
    pub updated_at_ledger: u32,
    pub anchor_transaction_id: String,
    pub callback_type: CallbackType,
    pub callback_status: String,
    pub stellar_tx_hash: String,
    pub failure_reason: String,
}

// ─── Incoming webhook payload ─────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug)]
pub struct CallbackPayload {
    pub transaction_id: String,
    pub stellar_account: String,
    pub amount: i128,
    pub asset_code: String,
    pub asset_issuer: String,
    pub idempotency_key: String,
    pub anchor_transaction_id: String,
    pub callback_type: CallbackType,
    pub callback_status: String,
}

// ─── Storage keys ─────────────────────────────────────────────────────────────

/// Discriminants used as ledger storage keys.
#[contracttype]
#[derive(Clone, Debug)]
pub enum StorageKey {
    Initialised,
    Admin,
    RelaySigner,
    RelaySigners,
    RelayThreshold,
    Paused,
    Transaction(String),
    TransactionById(u64),
    TransactionTags(u64),
    IdempotencyKey(String),
    PendingAdmin,
    SchemaVersion,
    TreasuryAccrued,
    RelayApprovals(u64),
    Relayer(Address),
    StatusIndex(TransactionStatus),
    UpgradeSnapshot,
    AnchorAmountCeiling(String),
    GlobalMaxAmount,
    UpgradeSnapshotKey,
    MergedInto(String),

    // ── Upgrade history (#86) ────────────────────────────────────────────────
    /// Singleton: bounded ring-buffer of [`UpgradeRecord`] entries.
    UpgradeHistory,
    /// Singleton: WASM hash currently installed on this contract.
    CurrentWasmHash,

    // ── Wave 2: Param Registry (#146) ───────────────────────────────────────
    /// Per-parameter record keyed by param name string.
    Param(String),

    // ── Wave 2: Collateral Bonding (#143) ───────────────────────────────────
    BondRecord(Address),
    UnbondRequest(Address),

    // ── Wave 2: Anchor Rebate (#145) ─────────────────────────────────────────
    AnchorTier(Address),

    // ── Amount ceilings (#169) ───────────────────────────────────────────────
    /// Per-anchor maximum registrable amount, keyed by `asset_issuer`.
    AnchorCeiling(String),

    // ── Disputes (#166) ──────────────────────────────────────────────────────
    /// Per-transaction open-dispute record. Present only while disputed.
    Dispute(String),
    /// Singleton: open disputes ordered by seq (oldest first).
    DisputeQueue,
    /// Singleton: monotonically increasing dispute sequence counter.
    DisputeSeq,
}

/// Alias kept for modules that import `DataKey` by that name.
pub type DataKey = StorageKey;

// ─── Wave 2: Param Registry (#146) ────────────────────────────────────────────

/// A single on-chain parameter entry.
///
/// Stored in persistent ledger storage keyed by [`StorageKey::Param`].
///
/// Values are `i128` scaled integers. Rate-like params use basis points
/// (10 000 = 100 %). Count-like params use their natural unit.
///
/// | Key                    | Semantics                                         |
/// |------------------------|---------------------------------------------------|
/// | `unbond_delay_ledgers` | Ledgers to wait before claiming unbond            |
/// | `slash_bps`            | Slash percentage in basis points                  |
/// | `base_fee_bps`         | Protocol base fee in basis points                 |
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ParamEntry {
    /// `i128` scaled integer value.
    pub value: i128,
    /// Ledger sequence at which this param was last updated.
    pub updated_at_ledger: u32,
    /// Address that last set this param (always the admin).
    pub updated_by: Address,
}

// ─── Upgrade history (#86) ────────────────────────────────────────────────────

/// A single on-chain record of a contract upgrade.
///
/// Stored as an element of [`StorageKey::UpgradeHistory`].
/// The vector is bounded at [`MAX_UPGRADE_HISTORY`] entries; oldest evicted
/// first (FIFO). Append logic is issue #27; entry-point wiring is issue #100.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeRecord {
    /// WASM hash before this upgrade.
    pub previous_wasm_hash: soroban_sdk::BytesN<32>,
    /// WASM hash installed by this upgrade.
    pub new_wasm_hash: soroban_sdk::BytesN<32>,
    /// On-chain schema version at upgrade time.
    pub schema_version: u32,
    /// Ledger sequence of the upgrade.
    pub ledger: u32,
    /// Admin that authorised this upgrade.
    pub admin: Address,
}

// ─── simulate_upgrade result type (#98) ──────────────────────────────────────

/// Verdict returned by the `simulate_upgrade` dry-run entry point (issue #98).
///
/// Each variant maps 1-to-1 to one guard in the real `upgrade()` flow,
/// evaluated cheapest-first so the two code paths share identical logic.
/// `simulate_upgrade` is **read-only** — it must never write state.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpgradeCompatibility {
    /// All guards pass; upgrade may proceed.
    Compatible,
    /// Contract not yet initialised.
    NotInitialised,
    /// Caller is not the admin.
    CallerNotAdmin,
    /// `expected_schema_version` did not match the on-chain value (guard F-04).
    SchemaVersionMismatch,
    /// Target schema outside the configured `[min, max]` range (issue #104).
    SchemaVersionOutOfRange,
    /// Required migration not registered in this WASM (issue #106).
    MigrationNotRegistered,
}

// ─── Disputes (#166) ──────────────────────────────────────────────────────────

/// An open dispute against a `Completed` transaction.
///
/// Stored in persistent storage keyed by [`StorageKey::Dispute`] and removed
/// when resolved. The typical dispute path is:
/// `register_callback` → `start_processing` → `complete_transaction` →
/// `dispute_transaction`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisputeRecord {
    /// Stable queue-position / pagination cursor.
    /// Assigned from [`StorageKey::DisputeSeq`] at raise time; never changes.
    pub seq: u64,
    /// Short reason code supplied by the caller
    /// (e.g. `"amount_mismatch"`, `"missing_settlement"`).
    pub reason: String,
    /// Relay signer or admin that raised the dispute.
    pub raised_by: Address,
    /// Ledger sequence at which the dispute was raised.
    pub raised_at_ledger: u32,
    /// Ledger timestamp (seconds) at which the dispute was raised.
    pub raised_at_timestamp: u64,
}

/// One entry of the open-dispute queue, ordered by `seq` (oldest first).
///
/// Stored as elements of [`StorageKey::DisputeQueue`].
/// `seq` doubles as a stable pagination cursor for `get_dispute_queue`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisputeQueueEntry {
    /// Stable ordering key; assigned from [`StorageKey::DisputeSeq`].
    pub seq: u64,
    /// The disputed transaction id.
    pub tx_id: String,
}

// ─── Wave 2: Collateral Bonding (#143) ────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug)]
pub struct BondRecord {
    pub signer: Address,
    pub amount: i128,
    pub bonded_at_ledger: u32,
    pub updated_at_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct UnbondRequest {
    pub amount: i128,
    pub requested_at_ledger: u32,
    pub claimable_at_ledger: u32,
}

// ─── Wave 2: Slash Evidence (#144) ────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug)]
pub struct SlashEvidence {
    pub tx_id: String,
    pub payload_a: CallbackPayload,
    pub payload_b: CallbackPayload,
}

// ─── Wave 2: Anchor Tier / Rebate (#145) ──────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AnchorTierConfig {
    pub anchor: Address,
    pub rebate_bps: u32,
    pub label: String,
    pub updated_at_ledger: u32,
}

// ─── Treasury (#142) ──────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreasuryConfig {
    pub party_a: Address,
    pub party_b: Address,
    pub epoch_cap: i128,
    pub epoch_length_ledgers: u32,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialised = 1,
    NotInitialised = 2,

    Unauthorised = 10,
    NotRelaySigner = 11,
    ContractPaused = 12,
    NoPendingAdminTransfer = 13,
    InvalidAdminNominee = 14,

    InvalidStellarAccount = 20,
    InvalidAmount = 21,
    InvalidAssetCode = 22,
    InvalidAssetIssuer = 23,
    MissingIdempotencyKey = 24,
    StringTooLong = 25,
    InvalidPayload = 26,

    TransactionNotFound = 30,
    InvalidStatusTransition = 31,

    DuplicateRequest = 40,

    StorageError = 50,

    SchemaVersionMismatch = 60,
    UnknownMigration = 61,
    MigrationFailed = 62,
    UpgradeNotReversible = 63,
    SelfCheckFailed = 64,

    InvalidParamName = 70,
    InvalidParamValue = 71,
    ParamNotFound = 72,

    InvalidBondAmount = 80,
    InsufficientBond = 81,
    UnbondAlreadyPending = 82,
    UnbondDelayNotElapsed = 83,
    NoPendingUnbond = 84,
    TimelockNotElapsed = 85,

    EvidenceTxIdMismatch = 90,
    EvidenceNotConflicting = 91,
    SignerNotBonded = 92,
    InvalidSlashEvidence = 93,

    InvalidRebateBps = 100,
    InvalidTierLabel = 101,
    AnchorTierNotFound = 102,

    AmountCeilingExceeded = 110,

    TreasuryNotConfigured = 120,
    EpochCapExceeded = 121,
    InsufficientTreasuryBalance = 122,
    SamePartyWithdrawal = 123,
    WithdrawalNotFound = 124,

    SignerAlreadyExists = 130,

    BatchTooLarge = 140,
    BatchWriteBudgetExceeded = 141,
    BatchEventBudgetExceeded = 142,

    // ── Disputes (#166) ──────────────────────────────────────────────────────
    /// `dispute_transaction` called on a tx already under dispute.
    AlreadyDisputed = 150,
    /// `resolve_dispute` / `get_dispute` called on a tx not under dispute.
    NotDisputed = 151,
    /// [`MAX_OPEN_DISPUTES`] cap reached.
    DisputeQueueFull = 152,
    /// Pagination `limit` was zero or exceeded `MAX_PAGE_LIMIT`.
    InvalidPageLimit = 153,
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
            assert_eq!(loaded, entry);
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
            assert_eq!(loaded, entry);
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
            assert_eq!(loaded, entry);
        }
    }

    #[test]
    fn param_name_max_len_is_32() {
        assert_eq!(MAX_PARAM_NAME_LEN, 32);
    }

    // ── UpgradeRecord round-trip (#86) ────────────────────────────────────────

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

    #[test]
    fn max_upgrade_history_is_32() {
        assert_eq!(MAX_UPGRADE_HISTORY, 32);
    }

    // ── UpgradeCompatibility (#98) ────────────────────────────────────────────

    #[test]
    fn upgrade_compatibility_all_variants_round_trip() {
        let env = Env::default();
        let variants = [
            UpgradeCompatibility::Compatible,
            UpgradeCompatibility::NotInitialised,
            UpgradeCompatibility::CallerNotAdmin,
            UpgradeCompatibility::SchemaVersionMismatch,
            UpgradeCompatibility::SchemaVersionOutOfRange,
            UpgradeCompatibility::MigrationNotRegistered,
        ];
        for (i, variant) in variants.iter().enumerate() {
            let name = match i {
                0 => "c0",
                1 => "c1",
                2 => "c2",
                3 => "c3",
                4 => "c4",
                _ => "c5",
            };
            let key = StorageKey::Param(String::from_str(&env, name));
            env.storage().persistent().set(&key, variant);
            let loaded: UpgradeCompatibility = env.storage().persistent().get(&key).unwrap();
            assert_eq!(&loaded, variant);
        }
    }

    // ── DisputeRecord / DisputeQueueEntry round-trip (#166) ───────────────────

    #[test]
    fn dispute_record_round_trips() {
        let env = Env::default();
        let raiser = soroban_sdk::Address::generate(&env);
        let record = DisputeRecord {
            seq: 7,
            reason: String::from_str(&env, "amount_mismatch"),
            raised_by: raiser.clone(),
            raised_at_ledger: 1_000,
            raised_at_timestamp: 9_999_999,
        };
        let key = StorageKey::Dispute(String::from_str(&env, "tx-abc"));
        env.storage().persistent().set(&key, &record);
        let loaded: DisputeRecord = env.storage().persistent().get(&key).unwrap();
        assert_eq!(loaded.seq, 7);
        assert_eq!(loaded.reason, String::from_str(&env, "amount_mismatch"));
        assert_eq!(loaded.raised_by, raiser);
        assert_eq!(loaded.raised_at_ledger, 1_000);
        assert_eq!(loaded.raised_at_timestamp, 9_999_999);
    }

    #[test]
    fn dispute_queue_entry_round_trips() {
        let env = Env::default();
        let entry = DisputeQueueEntry {
            seq: 3,
            tx_id: String::from_str(&env, "tx-xyz"),
        };
        let mut queue: soroban_sdk::Vec<DisputeQueueEntry> = soroban_sdk::Vec::new(&env);
        queue.push_back(entry.clone());
        env.storage()
            .persistent()
            .set(&StorageKey::DisputeQueue, &queue);
        let loaded: soroban_sdk::Vec<DisputeQueueEntry> = env
            .storage()
            .persistent()
            .get(&StorageKey::DisputeQueue)
            .unwrap();
        assert_eq!(loaded.len(), 1);
        let r = loaded.get_unchecked(0);
        assert_eq!(r.seq, 3);
        assert_eq!(r.tx_id, String::from_str(&env, "tx-xyz"));
    }

    #[test]
    fn max_open_disputes_is_100() {
        assert_eq!(MAX_OPEN_DISPUTES, 100);
    }
}
