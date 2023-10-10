//! Constant and configs for hotshot state prover

use ark_ff::PrimeField;
use ethereum_types::U256;

/// Number of entries/keys in the stake table
pub const NUM_ENTRIES: usize = 1000;

/// convert a U256 to a field element.
pub(crate) fn u256_to_field<F: PrimeField>(v: &U256) -> F {
    let mut bytes = vec![0u8; 32];
    v.to_little_endian(&mut bytes);
    F::from_le_bytes_mod_order(&bytes)
}
