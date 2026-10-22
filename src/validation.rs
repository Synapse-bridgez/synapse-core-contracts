//! Input validation for the escrow contract.
//!
//! This module implements the validation layer described in `DECISIONS.md`:
//!
//! * **§2–§5 — SEP-23 strkey validation.** Full base32 decode + CRC16-XModem
//!   checksum verification for Stellar strkeys (`stellar_account` and
//!   `asset_issuer` fields). A cheap non-`G` prefix fast-reject runs *before*
//!   the full base32 decode, per the cheapest-first guard ordering restored in
//!   issue #50. The CRC16-XModem implementation uses a 16-entry nibble table
//!   (not a 256-entry byte table) to match the documented size/speed trade-off
//!   from `DECISIONS.md` §5.
//! * **§6 — String-length caps.** Every free-text payload field is capped at
//!   the values enforced in `COST_MODEL.md` §6. Length checks are cheap and run
//!   early in the guard sequence.
//! * **Partial completion.** [`validate_settled_amount`] guards
//!   `partial_complete_transaction` (issue #63) so a partial settlement can
//!   never exceed the originally requested amount.
//!
//! Option B (cheap prefix/length check only) was explicitly rejected in
//! `DECISIONS.md` §2 and is intentionally *not* available here, not even as a
//! feature-flagged fallback.

use crate::types::ContractError;

// ---------------------------------------------------------------------------
// String-length caps (COST_MODEL.md §6)
// ---------------------------------------------------------------------------

/// Maximum length, in bytes, of a `CallbackPayload` free-text field.
///
/// Enforced cap from `COST_MODEL.md` §6.
pub const MAX_CALLBACK_TEXT_LEN: u32 = 256;

/// Maximum length, in bytes, of a dispute reason string.
///
/// Enforced cap from `COST_MODEL.md` §6.
pub const MAX_DISPUTE_REASON_LEN: u32 = 256;

/// Maximum length, in bytes, of a cancel reason string.
///
/// Enforced cap from `COST_MODEL.md` §6.
pub const MAX_CANCEL_REASON_LEN: u32 = 256;

/// Maximum length, in bytes, of a tag text string.
///
/// Enforced cap from `COST_MODEL.md` §6.
pub const MAX_TAG_TEXT_LEN: u32 = 64;

// ---------------------------------------------------------------------------
// Strkey constants (SEP-23)
// ---------------------------------------------------------------------------

/// Length of a decoded strkey payload (version byte + 32-byte key).
const STRKEY_PAYLOAD_LEN: usize = 33;

/// Length of the CRC16-XModem checksum appended to a strkey payload.
const STRKEY_CHECKSUM_LEN: usize = 2;

/// Length of the raw bytes covered by the checksum.
const STRKEY_CHECKED_LEN: usize = STRKEY_PAYLOAD_LEN + STRKEY_CHECKSUM_LEN;

/// Total length of an encoded strkey (base32 of 35 bytes, no padding).
const STRKEY_ENCODED_LEN: usize = 56;

/// Version byte for a Stellar account (`G...`).
const VERSION_ACCOUNT: u8 = 6 << 3;

/// Version byte for an asset issuer (`G...`).
const VERSION_ASSET_ISSUER: u8 = 6 << 3;

/// Base32 alphabet used by SEP-23 strkeys (RFC 4648, uppercase).
const BASE32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

// ---------------------------------------------------------------------------
// CRC16-XModem (16-entry nibble table)
// ---------------------------------------------------------------------------

/// 16-entry nibble table for CRC16-XModem (polynomial `0x1021`).
///
/// Using a nibble table rather than a full 256-entry byte table keeps the
/// constant data small (+49 B WASM, per the `5f66065` perf note) while still
/// processing two nibbles per byte.
const CRC16_XMODEM_NIBBLE_TABLE: [u16; 16] = [
    0x0000, 0x1021, 0x2042, 0x3063, 0x4084, 0x50a5, 0x60c6, 0x70e7, 0x8108, 0x9129, 0xa14a, 0xb16b,
    0xc18c, 0xd1ad, 0xe1ce, 0xf1ef,
];

/// Computes the CRC16-XModem checksum over `data`.
///
/// Processes each byte as two nibbles (high then low) against the 16-entry
/// nibble table. This is the checksum algorithm SEP-23 specifies for strkeys.
fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        // High nibble.
        let hi = ((crc >> 12) as u8) ^ (byte >> 4);
        crc = (crc << 4) ^ CRC16_XMODEM_NIBBLE_TABLE[(hi & 0x0f) as usize];
        // Low nibble.
        let lo = ((crc >> 12) as u8) ^ (byte & 0x0f);
        crc = (crc << 4) ^ CRC16_XMODEM_NIBBLE_TABLE[(lo & 0x0f) as usize];
    }
    crc
}

// ---------------------------------------------------------------------------
// Base32 decode
// ---------------------------------------------------------------------------

/// Decodes an unpadded uppercase base32 string into `out`.
///
/// Returns `false` if the input contains a character outside the SEP-23
/// alphabet or if the decoded length does not fit `out`. SEP-23 strkeys are
/// always unpadded, so `=` is rejected.
fn base32_decode(input: &[u8], out: &mut [u8; STRKEY_CHECKED_LEN]) -> bool {
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    let mut written: usize = 0;

    for &c in input {
        let value = match base32_value(c) {
            Some(v) => v,
            None => return false,
        };
        buffer = (buffer << 5) | (value as u32);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            if written >= out.len() {
                return false;
            }
            out[written] = ((buffer >> bits) & 0xff) as u8;
            written += 1;
        }
    }

    written == out.len()
}

/// Maps a base32 character to its 5-bit value, or `None` if invalid.
fn base32_value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'2'..=b'7' => Some(c - b'2' + 26),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Strkey validation
// ---------------------------------------------------------------------------

/// Validates a Stellar strkey (`G...`) with full base32 + CRC16-XModem check.
///
/// Ordering is cheapest-first (issue #50):
/// 1. Length check (O(1)).
/// 2. Non-`G` prefix fast-reject (O(1)) — avoids the full decode for the
///    common case of a malformed or non-account strkey.
/// 3. Full base32 decode.
/// 4. Version-byte check.
/// 5. CRC16-XModem checksum verification.
///
/// Returns `Ok(())` if `key` is a well-formed account strkey, otherwise the
/// appropriate [`ContractError`].
fn validate_strkey(key: &str, expected_version: u8) -> Result<(), ContractError> {
    let bytes = key.as_bytes();

    // 1. Length check — cheap, runs first.
    if bytes.len() != STRKEY_ENCODED_LEN {
        return Err(ContractError::InvalidStrkey);
    }

    // 2. Non-'G' prefix fast-reject — cheap, before any decode work.
    if bytes[0] != b'G' {
        return Err(ContractError::InvalidStrkey);
    }

    // 3. Full base32 decode.
    let mut decoded = [0u8; STRKEY_CHECKED_LEN];
    if !base32_decode(bytes, &mut decoded) {
        return Err(ContractError::InvalidStrkey);
    }

    // 4. Version-byte check.
    if decoded[0] != expected_version {
        return Err(ContractError::InvalidStrkey);
    }

    // 5. CRC16-XModem checksum verification over the payload.
    let expected_crc = crc16_xmodem(&decoded[..STRKEY_PAYLOAD_LEN]);
    let actual_crc = ((decoded[STRKEY_PAYLOAD_LEN] as u16) << 8)
        | (decoded[STRKEY_PAYLOAD_LEN + 1] as u16);
    if expected_crc != actual_crc {
        return Err(ContractError::InvalidStrkey);
    }

    Ok(())
}

/// Validates a `stellar_account` field: a `G...` account strkey.
///
/// Performs the full SEP-23 base32 + CRC16-XModem check.
pub fn validate_stellar_account(key: &str) -> Result<(), ContractError> {
    validate_strkey(key, VERSION_ACCOUNT)
}

/// Validates an `asset_issuer` field: a `G...` account strkey.
///
/// Performs the full SEP-23 base32 + CRC16-XModem check.
pub fn validate_asset_issuer(key: &str) -> Result<(), ContractError> {
    validate_strkey(key, VERSION_ASSET_ISSUER)
}

// ---------------------------------------------------------------------------
// String-length caps
// ---------------------------------------------------------------------------

/// Enforces a byte-length cap on a free-text field.
///
/// Returns [`ContractError::StringTooLong`] if `value` exceeds `max_len`.
fn check_len(value: &str, max_len: u32) -> Result<(), ContractError> {
    if value.len() as u32 > max_len {
        return Err(ContractError::StringTooLong);
    }
    Ok(())
}

/// Validates a `CallbackPayload` free-text field against its cap.
///
/// Cap: [`MAX_CALLBACK_TEXT_LEN`] bytes (`COST_MODEL.md` §6).
pub fn validate_callback_text(value: &str) -> Result<(), ContractError> {
    check_len(value, MAX_CALLBACK_TEXT_LEN)
}

/// Validates a dispute reason against its cap.
///
/// Cap: [`MAX_DISPUTE_REASON_LEN`] bytes (`COST_MODEL.md` §6).
pub fn validate_dispute_reason(value: &str) -> Result<(), ContractError> {
    check_len(value, MAX_DISPUTE_REASON_LEN)
}

/// Validates a cancel reason against its cap.
///
/// Cap: [`MAX_CANCEL_REASON_LEN`] bytes (`COST_MODEL.md` §6).
pub fn validate_cancel_reason(value: &str) -> Result<(), ContractError> {
    check_len(value, MAX_CANCEL_REASON_LEN)
}

/// Validates a tag text against its cap.
///
/// Cap: [`MAX_TAG_TEXT_LEN`] bytes (`COST_MODEL.md` §6).
pub fn validate_tag_text(value: &str) -> Result<(), ContractError> {
    check_len(value, MAX_TAG_TEXT_LEN)
}

// ---------------------------------------------------------------------------
// Partial completion
// ---------------------------------------------------------------------------

/// Validates a partial-settlement amount against the original requested amount.
///
/// Enforces `0 < settled_amount <= original_amount`.
///
/// **Degenerate partial = full:** settling exactly the full original amount via
/// `partial_complete_transaction` is explicitly **allowed** (treated as a
/// degenerate partial that happens to equal the full amount). Callers that
/// intend a full completion should still prefer `complete_transaction`, but
/// this validator does not reject the boundary case.
///
/// Returns [`ContractError::InvalidSettledAmount`] on violation.
pub fn validate_settled_amount(
    settled_amount: i128,
    original_amount: i128,
) -> Result<(), ContractError> {
    // Cheapest-first: reject non-positive settlements before the comparison.
    if settled_amount <= 0 {
        return Err(ContractError::InvalidSettledAmount);
    }
    if settled_amount > original_amount {
        return Err(ContractError::InvalidSettledAmount);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests — DECISIONS.md §5 fixtures
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- DECISIONS.md §5 fixtures -------------------------------------------

    /// A valid `G...` account strkey fixture (SEP-23).
    const FIXTURE_VALID_ACCOUNT: &str =
        "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ";

    /// A valid `G...` asset-issuer strkey fixture (SEP-23).
    const FIXTURE_VALID_ISSUER: &str =
        "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";

    /// A strkey with a valid shape but a corrupted checksum.
    const FIXTURE_BAD_CHECKSUM: &str =
        "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGA";

    /// A strkey that is one character short (wrong length).
    const FIXTURE_WRONG_LENGTH: &str =
        "GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJV";

    /// A strkey with a non-`G` prefix (wrong network-byte / prefix).
    const FIXTURE_WRONG_PREFIX: &str =
        "SA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSGZ";

    // -- CRC16-XModem -------------------------------------------------------

    #[test]
    fn crc16_xmodem_known_vector() {
        // Standard CRC16-XModem check value for "123456789" is 0x31C3.
        assert_eq!(crc16_xmodem(b"123456789"), 0x31C3);
    }

    #[test]
    fn crc16_xmodem_empty_is_zero() {
        assert_eq!(crc16_xmodem(b""), 0x0000);
    }

    // -- Base32 -------------------------------------------------------------

    #[test]
    fn base32_rejects_invalid_characters() {
        let mut out = [0u8; STRKEY_CHECKED_LEN];
        assert!(!base32_decode(b"GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSG!", &mut out));
        // Lowercase is not valid SEP-23.
        assert!(!base32_decode(b"ga7qynf7sowq3glr2bgmzehxavirza4kvwltjjfc7mgxua74p7ujvsgz", &mut out));
        // Padding is not valid SEP-23.
        assert!(!base32_decode(b"GA7QYNF7SOWQ3GLR2BGMZEHXAVIRZA4KVWLTJJFC7MGXUA74P7UJVSG=", &mut out));
    }

    // -- Valid strkeys ------------------------------------------------------

    #[test]
    fn accepts_valid_account() {
        assert_eq!(validate_stellar_account(FIXTURE_VALID_ACCOUNT), Ok(()));
    }

    #[test]
    fn accepts_valid_asset_issuer() {
        assert_eq!(validate_asset_issuer(FIXTURE_VALID_ISSUER), Ok(()));
    }

    // -- Malformed checksum -------------------------------------------------

    #[test]
    fn rejects_malformed_checksum() {
        assert_eq!(
            validate_stellar_account(FIXTURE_BAD_CHECKSUM),
            Err(ContractError::InvalidStrkey)
        );
    }

    // -- Wrong length -------------------------------------------------------

    #[test]
    fn rejects_wrong_length_key() {
        assert_eq!(
            validate_stellar_account(FIXTURE_WRONG_LENGTH),
            Err(ContractError::InvalidStrkey)
        );
    }

    #[test]
    fn rejects_empty_key() {
        assert_eq!(
            validate_stellar_account(""),
            Err(ContractError::InvalidStrkey)
        );
    }

    // -- Wrong network-byte / prefix ---------------------------------------

    #[test]
    fn rejects_wrong_prefix() {
        assert_eq!(
            validate_stellar_account(FIXTURE_WRONG_PREFIX),
            Err(ContractError::InvalidStrkey)
        );
    }

    // -- String-length caps -------------------------------------------------

    #[test]
    fn callback_text_at_cap_accepted() {
        let s = "a".repeat(MAX_CALLBACK_TEXT_LEN as usize);
        assert_eq!(validate_callback_text(&s), Ok(()));
    }

    #[test]
    fn callback_text_over_cap_rejected() {
        let s = "a".repeat(MAX_CALLBACK_TEXT_LEN as usize + 1);
        assert_eq!(validate_callback_text(&s), Err(ContractError::StringTooLong));
    }

    #[test]
    fn dispute_reason_at_cap_accepted() {
        let s = "a".repeat(MAX_DISPUTE_REASON_LEN as usize);
        assert_eq!(validate_dispute_reason(&s), Ok(()));
    }

    #[test]
    fn dispute_reason_over_cap_rejected() {
        let s = "a".repeat(MAX_DISPUTE_REASON_LEN as usize + 1);
        assert_eq!(validate_dispute_reason(&s), Err(ContractError::StringTooLong));
    }

    #[test]
    fn cancel_reason_at_cap_accepted() {
        let s = "a".repeat(MAX_CANCEL_REASON_LEN as usize);
        assert_eq!(validate_cancel_reason(&s), Ok(()));
    }

    #[test]
    fn cancel_reason_over_cap_rejected() {
        let s = "a".repeat(MAX_CANCEL_REASON_LEN as usize + 1);
        assert_eq!(validate_cancel_reason(&s), Err(ContractError::StringTooLong));
    }

    #[test]
    fn tag_text_at_cap_accepted() {
        let s = "a".repeat(MAX_TAG_TEXT_LEN as usize);
        assert_eq!(validate_tag_text(&s), Ok(()));
    }

    #[test]
    fn tag_text_over_cap_rejected() {
        let s = "a".repeat(MAX_TAG_TEXT_LEN as usize + 1);
        assert_eq!(validate_tag_text(&s), Err(ContractError::StringTooLong));
    }

    // -- Settled amount -----------------------------------------------------

    #[test]
    fn settled_amount_under_original_accepted() {
        assert_eq!(validate_settled_amount(50, 100), Ok(()));
    }

    #[test]
    fn settled_amount_at_original_accepted() {
        // Degenerate partial = full is explicitly allowed.
        assert_eq!(validate_settled_amount(100, 100), Ok(()));
    }

    #[test]
    fn settled_amount_over_original_rejected() {
        assert_eq!(
            validate_settled_amount(101, 100),
            Err(ContractError::InvalidSettledAmount)
        );
    }

    #[test]
    fn settled_amount_zero_rejected() {
        assert_eq!(
            validate_settled_amount(0, 100),
            Err(ContractError::InvalidSettledAmount)
        );
    }

    #[test]
    fn settled_amount_negative_rejected() {
        assert_eq!(
            validate_settled_amount(-1, 100),
            Err(ContractError::InvalidSettledAmount)
        );
    }
}