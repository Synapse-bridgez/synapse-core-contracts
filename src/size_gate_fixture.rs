//! Deliberately bloated dummy addition for the WASM size gate's CI self-test
//! (#122).
//!
//! Compiled only with `--features size-gate-fixture` on a wasm target. It
//! exports a function indexing a [`BLOAT_LEN`]-byte table of pseudo-random
//! bytes, so neither LTO nor `opt-level = "z"` can fold it away and the
//! release WASM grows by roughly that much — well past the gate's growth
//! threshold. The resulting WASM is only measured, never deployed.

/// Size of the bloat table; comfortably above `max_growth_pct` of the
/// baseline in `wasm_size.toml`.
const BLOAT_LEN: usize = 16 * 1024;

/// Pseudo-random (LCG) bytes, so the data segment can't be elided as zeros.
static BLOAT: [u8; BLOAT_LEN] = {
    let mut table = [0u8; BLOAT_LEN];
    let mut state: u32 = 0x9E37_79B9;
    let mut i = 0;
    while i < BLOAT_LEN {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        table[i] = (state >> 24) as u8;
        i += 1;
    }
    table
};

/// Exported so the table stays reachable after dead-code elimination.
#[no_mangle]
pub extern "C" fn size_gate_fixture(index: u32) -> u32 {
    BLOAT[index as usize % BLOAT_LEN] as u32
}
