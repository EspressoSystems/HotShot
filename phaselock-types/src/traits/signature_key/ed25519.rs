//! Demonstration implementation of the [`SignatureKey`] trait using ed25519
use super::{EncodedPublicKey, EncodedSignature, SignatureKey, TestableSignatureKey};
use ed25519_compact::{KeyPair, Noise, PublicKey, SecretKey, Seed, Signature};
use serde::{de::Error, Deserialize, Serialize};
use std::cmp::Ordering;
use tagged_base64::TaggedBase64;
use tracing::{debug, instrument, warn};
use espresso_systems_common::phaselock::PEER_ID;

/// Private key type for a ed25519 [`SignatureKey`] pair
#[derive(PartialEq, Eq, Clone)]
pub struct Ed25519Priv {
    /// The private key for  this keypair
    priv_key: SecretKey,
}

impl Ed25519Priv {
    /// Generate a new private key from scratch
    pub fn generate() -> Self {
        let key_pair = KeyPair::generate();
        let priv_key = key_pair.sk;
        Self { priv_key }
    }

    /// Generate a new private key from a seed
    pub fn generate_from_seed(seed: [u8; 32]) -> Self {
        let key_pair = KeyPair::from_seed(Seed::new(seed));
        let priv_key = key_pair.sk;
        Self { priv_key }
    }

    /// Generate a new private key from a seed and a number
    ///
    /// Hashes the seed and the number together using blake3. This method is
    /// useful for testing
    pub fn generated_from_seed_indexed(seed: [u8; 32], index: u64) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&seed);
        hasher.update(&index.to_le_bytes());
        let new_seed = *hasher.finalize().as_bytes();
        Self::generate_from_seed(new_seed)
    }

    /// Create an existing private key from bytes
    #[instrument]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        match SecretKey::from_slice(bytes) {
            Ok(priv_key) => Some(Self { priv_key }),
            Err(e) => {
                warn!(?e, "Failed to decode private key");
                None
            }
        }
    }

    /// Convert a private key to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        self.priv_key.to_vec()
    }
}

impl PartialOrd for Ed25519Priv {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let self_bytes = self.priv_key.as_ref();
        let other_bytes = other.priv_key.as_ref();
        self_bytes.partial_cmp(other_bytes)
    }
}

impl Ord for Ed25519Priv {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_bytes = self.priv_key.as_ref();
        let other_bytes = other.priv_key.as_ref();
        self_bytes.cmp(other_bytes)
    }
}

/// Public key type for an ed25519 [`SignatureKey`] pair
///
/// This type makes use of noise for non-determinisitc signatures.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Ed25519Pub {
    /// The public key for this keypair
    pub_key: PublicKey,
}

impl std::fmt::Debug for Ed25519Pub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Ed25519Pub")
            .field(&tagged_base64::to_string(&self.to_tagged_base64()))
            .finish()
    }
}

impl PartialOrd for Ed25519Pub {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let self_bytes = self.pub_key.as_ref();
        let other_bytes = other.pub_key.as_ref();
        self_bytes.partial_cmp(other_bytes)
    }
}

impl Ord for Ed25519Pub {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_bytes = self.pub_key.as_ref();
        let other_bytes = other.pub_key.as_ref();
        self_bytes.cmp(other_bytes)
    }
}

impl Ed25519Pub {
    /// Return the [`TaggedBase64`] representation of this key.
    #[allow(clippy::missing_panics_doc)] // `TaggedBase64::new()` only panics if `PEER_ID` is not valid base64, which it is.
    pub fn to_tagged_base64(&self) -> TaggedBase64 {
        TaggedBase64::new(PEER_ID, self.pub_key.as_ref()).unwrap()
    }
}

impl SignatureKey for Ed25519Pub {
    type PrivateKey = Ed25519Priv;

    #[instrument(skip(self))]
    fn validate(&self, signature: &EncodedSignature, data: &[u8]) -> bool {
        let signature = &signature.0[..];
        // Convert to the signature type
        match Signature::from_slice(signature) {
            Ok(signature) => {
                // Validate the signature
                match self.pub_key.verify(data, &signature) {
                    Ok(_) => true,
                    Err(e) => {
                        debug!(?e, "Signature failed verification");
                        false
                    }
                }
            }
            Err(e) => {
                // Log and error
                debug!(?e, "signature was structurally invalid");
                false
            }
        }
    }

    fn sign(private_key: &Self::PrivateKey, data: &[u8]) -> EncodedSignature {
        // Generate some noise first
        let noise = Noise::generate();
        // Perform the signature
        let signature = private_key.priv_key.sign(data, Some(noise));
        // Convert the signature to bytes and return
        EncodedSignature(signature.to_vec())
    }

    fn from_private(private_key: &Self::PrivateKey) -> Self {
        let pub_key = private_key.priv_key.public_key();
        Self { pub_key }
    }

    fn to_bytes(&self) -> EncodedPublicKey {
        EncodedPublicKey(self.pub_key.to_vec())
    }

    #[instrument]
    fn from_bytes(bytes: &EncodedPublicKey) -> Option<Self> {
        let bytes = &bytes.0[..];
        match PublicKey::from_slice(bytes) {
            Ok(pub_key) => Some(Self { pub_key }),
            Err(e) => {
                debug!(?e, "Failed to deserialize public key");
                None
            }
        }
    }
}

impl TestableSignatureKey for Ed25519Pub {
    fn generate_test_key(id: u64) -> Self::PrivateKey {
        Ed25519Priv::generated_from_seed_indexed([0_u8; 32], id)
    }
}

impl Serialize for Ed25519Pub {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let base64 = self.to_tagged_base64();
        serializer.serialize_str(&tagged_base64::to_string(&base64))
    }
}

impl<'de> Deserialize<'de> for Ed25519Pub {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use std::str::FromStr;

        let base64 = String::deserialize(deserializer)?;
        let base64 =
            TaggedBase64::from_str(&base64).map_err(|e| D::Error::custom(e.to_string()))?;
        if base64.tag() != PEER_ID {
            return Err(D::Error::custom(format!(
                "Invalid Ed25519Pub tag: {:?}",
                base64.tag()
            )));
        }

        if let Some(key) = Self::from_bytes(&EncodedPublicKey(base64.value())) {
            Ok(key)
        } else {
            Err(D::Error::custom("Failed to decode Ed25519 key"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;

    // Basic smoke test
    #[test]
    fn signature_should_validate() {
        // Get some data to test sign with
        let mut data = [0_u8; 64];
        rand::thread_rng().fill_bytes(&mut data);

        // Get a key to sign it with
        let priv_key = Ed25519Priv::generate();
        // And the matching public key
        let pub_key = Ed25519Pub::from_private(&priv_key);

        // Sign the data with it
        let signature = Ed25519Pub::sign(&priv_key, &data);
        // Verify the signature
        assert!(pub_key.validate(&signature, &data));
    }

    // Make sure serialization round trip works
    #[test]
    fn serialize_key() {
        // Get a private key
        let priv_key = Ed25519Priv::generate();
        // And the matching public key
        let pub_key = Ed25519Pub::from_private(&priv_key);

        // Convert the private key to bytes and back, then verify equality
        let priv_key_bytes = priv_key.to_bytes();
        let priv_key_2 = Ed25519Priv::from_bytes(&priv_key_bytes).expect("Failed to deser key");
        assert!(priv_key == priv_key_2);

        // Convert the public key to bytes and back, then verify equality
        let pub_key_bytes = pub_key.to_bytes();
        let pub_key_2 = Ed25519Pub::from_bytes(&pub_key_bytes).expect("Failed to deser key");
        assert!(pub_key == pub_key_2);

        // Serialize the public key and back, then verify equality
        let serialized = serde_json::to_string(&pub_key).expect("Failed to ser key");
        let pub_key_2: Ed25519Pub = serde_json::from_str(&serialized).expect("Failed to deser key");
        assert!(pub_key == pub_key_2);
    }

    #[test]
    fn base64_deserialize() {
        let valid = r#""PEER_ID~oUla6NPfKBahJVNpwlxO5UeHuwLySBnt4a3L2GR-jHla""#;
        assert!(serde_json::from_str::<Ed25519Pub>(valid).is_ok());

        for invalid in [
            r#""PEERID~oUla6NPfKBahJVNpwlxO5UeHuwLySBnt4a3L2GR-jHla""#, // invalid tag
            r#""PEER_ID~oUla6NPfKBahJVNpwlxO5UeHuwLySBnt4a3L2GR-jHlb""#, // invalid checksum
        ] {
            assert!(serde_json::from_str::<Ed25519Pub>(invalid).is_err());
        }
    }
}
