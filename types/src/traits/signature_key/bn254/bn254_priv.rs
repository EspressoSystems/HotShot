use custom_debug::Debug;
use espresso_systems_common::hotshot::tag::PRIVKEY_ID;
use serde::{de::Error, Deserialize, Serialize};
use std::{cmp::Ordering, fmt, str::FromStr};
use tagged_base64::TaggedBase64;
use tracing::{instrument, warn};
use hotshot_primitives::qc::bit_vector::BitVectorQC;
use jf_primitives::signatures::bls_over_bn254::{BLSOverBN254CurveSignatureScheme, KeyPair as QCKeyPair, VerKey, SignKey as QCSignKey};
use rand_chacha::ChaCha20Rng;
use rand::SeedableRng;

/// Private key type for a bn254 keypair
#[derive(PartialEq, Eq, Clone, Serialize, Deserialize, Debug)]
pub struct BN254Priv {
    /// The private key for  this keypair
    pub(super) priv_key: QCSignKey,
}

impl BN254Priv {
    /// Generate a new private key from scratch
    #[must_use]
    pub fn generate() -> Self {
        let key_pair = QCKeyPair::generate(&mut rand::thread_rng());
        let priv_key = key_pair.sign_key_ref();
        Self { priv_key: priv_key.clone() }
    }

    /// Get real seed used for random key generation funtion
    pub fn get_seed_from_seed_indexed(seed: [u8; 32], index: u64) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&seed);
        hasher.update(&index.to_le_bytes());
        let new_seed = *hasher.finalize().as_bytes();
        new_seed
    }

    /// Generate a new private key from a seed
    #[must_use]
    pub fn generate_from_seed(seed: [u8; 32]) -> Self {
        let key_pair = QCKeyPair::generate(&mut ChaCha20Rng::from_seed(seed));
        let priv_key = key_pair.sign_key_ref();
        Self { priv_key: priv_key.clone() }
    }

    /// Generate a new private key from a seed and a number
    ///
    /// Hashes the seed and the number together using blake3. This method is
    /// useful for testing
    #[must_use]
    pub fn generated_from_seed_indexed(seed: [u8; 32], index: u64) -> Self {
        let new_seed = Self::get_seed_from_seed_indexed(seed, index);
        Self::generate_from_seed(new_seed)
    }

}

impl PartialOrd for BN254Priv {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let self_bytes = &self.priv_key.to_string();
        let other_bytes = &other.priv_key.to_string();
        self_bytes.partial_cmp(other_bytes)
    }
}

impl Ord for BN254Priv {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_bytes = &self.priv_key.to_string();
        let other_bytes = &other.priv_key.to_string();
        self_bytes.cmp(other_bytes)
    }
}
