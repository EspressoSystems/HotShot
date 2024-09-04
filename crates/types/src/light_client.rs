// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! Types and structs associated with light client state

use std::collections::HashMap;

use ark_ed_on_bn254::EdwardsConfig as Config;
use ark_ff::PrimeField;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ethereum_types::U256;
use jf_signature::schnorr;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use tagged_base64::tagged;

/// Base field in the prover circuit
pub type CircuitField = ark_ed_on_bn254::Fq;
/// Concrete type for light client state
pub type LightClientState = GenericLightClientState<CircuitField>;
/// Signature scheme
pub type StateSignatureScheme =
jf_signature::schnorr::SchnorrSignatureScheme<ark_ed_on_bn254::EdwardsConfig>;
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

/// Stake table state
pub type StakeState = GenericStakeState<CircuitField>;

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
}

impl<F: PrimeField> From<GenericLightClientState<F>> for [F; 3] {
    fn from(state: GenericLightClientState<F>) -> Self {
        [
            F::from(state.view_number as u64),
            F::from(state.block_height as u64),
            state.block_comm_root,
        ]
    }
}

pub struct GenericStakeState<F: PrimeField> {
    /// threshold
    pub threshold: F,

    /// Commitments to the table columns
    pub stake_table_bls_key_comm: F,
    pub stake_table_schnorr_key_comm: F,
    pub stake_table_amount_comm: F,
}

impl<F: PrimeField> From<&GenericLightClientState<F>> for [F; 3] {
    fn from(state: &GenericLightClientState<F>) -> Self {
        [
            F::from(state.view_number as u64),
            F::from(state.block_height as u64),
            state.block_comm_root,
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
    /// Return the view number of the light client state
    #[must_use]
    pub fn view_number(&self) -> F {
        self.0[0]
    }

    /// Return the block height of the light client state
    #[must_use]
    pub fn block_height(&self) -> F {
        self.0[1]
    }

    /// Return the block commitment root of the light client state
    #[must_use]
    pub fn block_comm_root(&self) -> F {
        self.0[2]
    }
}
