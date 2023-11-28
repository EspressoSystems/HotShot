//! Types and structs associated with light client state

use ark_ff::PrimeField;

/// A serialized light client state for proof generation
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, Default)]
pub struct LightClientState<F: PrimeField> {
    /// Current view number
    pub view_number: usize,
    /// Current block height
    pub block_height: usize,
    /// Root of the block commitment tree
    pub block_comm_root: F,
    /// Commitment for fee ledger
    pub fee_ledger_comm: F,
    /// Commitment for the stake table
    pub stake_table_comm: (F, F, F),
}

impl<F: PrimeField> From<LightClientState<F>> for [F; 7] {
    fn from(state: LightClientState<F>) -> Self {
        [
            F::from(state.view_number as u64),
            F::from(state.block_height as u64),
            state.block_comm_root,
            state.fee_ledger_comm,
            state.stake_table_comm.0,
            state.stake_table_comm.1,
            state.stake_table_comm.2,
        ]
    }
}
impl<F: PrimeField> From<&LightClientState<F>> for [F; 7] {
    fn from(state: &LightClientState<F>) -> Self {
        [
            F::from(state.view_number as u64),
            F::from(state.block_height as u64),
            state.block_comm_root,
            state.fee_ledger_comm,
            state.stake_table_comm.0,
            state.stake_table_comm.1,
            state.stake_table_comm.2,
        ]
    }
}

use ark_ed_on_bn254::EdwardsConfig as Config;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

/// Verification key for verifying state signatures
pub type StateVerKey = jf_primitives::signatures::schnorr::VerKey<Config>;
/// Signing key for signing a light client state
pub type StateSignKey = jf_primitives::signatures::schnorr::SignKey<ark_ed_on_bn254::Fr>;
/// Key pairs for signing/verifying a light client state
pub type StateSigKeyPairs = jf_primitives::signatures::schnorr::KeyPair<Config>;
/// Signatures
pub type StateSignature = jf_primitives::signatures::schnorr::Signature<Config>;

/// Generate key pairs from seed
#[must_use]
pub fn generate_state_key_pair_from_seed(seed: [u8; 32]) -> StateSigKeyPairs {
    StateSigKeyPairs::generate(&mut ChaCha20Rng::from_seed(seed))
}

/// Generate key pairs from an index and a seed
#[must_use]
pub fn generate_state_key_pair_from_seed_indexed(seed: [u8; 32], index: u64) -> StateSigKeyPairs {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&seed);
    hasher.update(&index.to_le_bytes());
    let new_seed = *hasher.finalize().as_bytes();
    generate_state_key_pair_from_seed(new_seed)
}
