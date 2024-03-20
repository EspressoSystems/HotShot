//! Types and structs associated with light client state

use ark_ed_on_bn254::EdwardsConfig as Config;
use ark_ff::PrimeField;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ethereum_types::U256;
use jf_primitives::signatures::schnorr;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tagged_base64::tagged;

/// Base field in the prover circuit
pub type CircuitField = ark_ed_on_bn254::Fq;
/// Concrete type for light client state
pub type LightClientState = GenericLightClientState<CircuitField>;
/// Signature scheme
pub type StateSignatureScheme =
    jf_primitives::signatures::schnorr::SchnorrSignatureScheme<ark_ed_on_bn254::EdwardsConfig>;
/// Signatures
pub type StateSignature = schnorr::Signature<Config>;
/// Verification key for verifying state signatures
pub type StateVerKey = schnorr::VerKey<Config>;
/// Signing key for signing a light client state
pub type StateSignKey = schnorr::SignKey<ark_ed_on_bn254::Fr>;
/// Concrete for circuit's public input
pub type PublicInput = GenericPublicInput<CircuitField>;
/// Key pairs for signing/verifying a light client state
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct StateKeyPair(pub schnorr::KeyPair<Config>);

/// Request body to send to the state relay server
#[derive(Clone, Debug, CanonicalSerialize, CanonicalDeserialize, Serialize, Deserialize)]
pub struct StateSignatureRequestBody {
    /// The public key associated with this request
    pub key: StateVerKey,
    /// The associated light client state
    pub state: LightClientState,
    /// The associated signature of the light client state
    pub signature: StateSignature,
}

/// The state signatures bundle is a light client state and its signatures collected
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StateSignaturesBundle {
    /// The state for this signatures bundle
    pub state: LightClientState,
    /// The collected signatures
    pub signatures: HashMap<StateVerKey, StateSignature>,
    /// Total stakes associated with the signer
    pub accumulated_weight: U256,
}

/// A light client state
#[tagged("LIGHT_CLIENT_STATE")]
#[derive(
    Clone,
    Debug,
    CanonicalSerialize,
    CanonicalDeserialize,
    Default,
    Eq,
    PartialEq,
    PartialOrd,
    Ord,
    Hash,
)]
pub struct GenericLightClientState<F: PrimeField> {
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

impl<F: PrimeField> From<GenericLightClientState<F>> for [F; 7] {
    fn from(state: GenericLightClientState<F>) -> Self {
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
impl<F: PrimeField> From<&GenericLightClientState<F>> for [F; 7] {
    fn from(state: &GenericLightClientState<F>) -> Self {
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

impl std::ops::Deref for StateKeyPair {
    type Target = schnorr::KeyPair<Config>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl StateKeyPair {
    /// Generate key pairs from private signing keys
    #[must_use]
    pub fn from_sign_key(sk: StateSignKey) -> Self {
        Self(schnorr::KeyPair::<Config>::from(sk))
    }

    /// Generate key pairs from `thread_rng()`
    #[must_use]
    pub fn generate() -> StateKeyPair {
        schnorr::KeyPair::generate(&mut rand::thread_rng()).into()
    }

    /// Generate key pairs from seed
    #[must_use]
    pub fn generate_from_seed(seed: [u8; 32]) -> StateKeyPair {
        schnorr::KeyPair::generate(&mut ChaCha20Rng::from_seed(seed)).into()
    }

    /// Generate key pairs from an index and a seed
    #[must_use]
    pub fn generate_from_seed_indexed(seed: [u8; 32], index: u64) -> StateKeyPair {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&seed);
        hasher.update(&index.to_le_bytes());
        let new_seed = *hasher.finalize().as_bytes();
        Self::generate_from_seed(new_seed)
    }
}

impl From<schnorr::KeyPair<Config>> for StateKeyPair {
    fn from(value: schnorr::KeyPair<Config>) -> Self {
        StateKeyPair(value)
    }
}

/// Public input to the light client state prover service
#[derive(Clone, Debug)]
pub struct GenericPublicInput<F: PrimeField>(Vec<F>);

impl<F: PrimeField> AsRef<[F]> for GenericPublicInput<F> {
    fn as_ref(&self) -> &[F] {
        &self.0
    }
}

impl<F: PrimeField> From<Vec<F>> for GenericPublicInput<F> {
    fn from(v: Vec<F>) -> Self {
        Self(v)
    }
}

impl<F: PrimeField> GenericPublicInput<F> {
    /// Return the threshold
    #[must_use]
    pub fn threshold(&self) -> F {
        self.0[0]
    }

    /// Return the view number of the light client state
    #[must_use]
    pub fn view_number(&self) -> F {
        self.0[1]
    }

    /// Return the block height of the light client state
    #[must_use]
    pub fn block_height(&self) -> F {
        self.0[2]
    }

    /// Return the block commitment root of the light client state
    #[must_use]
    pub fn block_comm_root(&self) -> F {
        self.0[3]
    }

    /// Return the fee ledger commitment of the light client state
    #[must_use]
    pub fn fee_ledger_comm(&self) -> F {
        self.0[4]
    }

    /// Return the stake table commitment of the light client state
    #[must_use]
    pub fn stake_table_comm(&self) -> (F, F, F) {
        (self.0[5], self.0[6], self.0[7])
    }

    /// Return the qc key commitment of the light client state
    #[must_use]
    pub fn qc_key_comm(&self) -> F {
        self.0[5]
    }

    /// Return the state key commitment of the light client state
    #[must_use]
    pub fn state_key_comm(&self) -> F {
        self.0[6]
    }

    /// Return the stake amount commitment of the light client state
    #[must_use]
    pub fn stake_amount_comm(&self) -> F {
        self.0[7]
    }
}
