use ed25519_compact::{KeyPair, SecretKey, Seed};
use espresso_systems_common::hotshot::tag::PRIVKEY_ID;
use serde::{de::Error, Deserialize, Serialize};
use std::{cmp::Ordering, fmt, str::FromStr};
use tagged_base64::TaggedBase64;
use tracing::{instrument, warn};

/// Private key type for a ed25519 keypair
#[derive(PartialEq, Eq, Clone)]
pub struct Ed25519Priv {
    /// The private key for  this keypair
    pub(super) priv_key: SecretKey,
}

impl Ed25519Priv {
    /// Generate a new private key from scratch
    #[must_use]
    pub fn generate() -> Self {
        let key_pair = KeyPair::generate();
        let priv_key = key_pair.sk;
        Self { priv_key }
    }

    /// Generate a new private key from a seed
    #[must_use]
    pub fn generate_from_seed(seed: [u8; 32]) -> Self {
        let key_pair = KeyPair::from_seed(Seed::new(seed));
        let priv_key = key_pair.sk;
        Self { priv_key }
    }

    /// Generate a new private key from a seed and a number
    ///
    /// Hashes the seed and the number together using blake3. This method is
    /// useful for testing
    #[must_use]
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
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.priv_key.to_vec()
    }

    /// Return the [`TaggedBase64`] representation of this key.
    #[allow(clippy::missing_panics_doc)] // `TaggedBase64::new()` only panics if `PRIVKEY_ID` is not valid base64, which it is.
    #[must_use]
    pub fn to_tagged_base64(&self) -> TaggedBase64 {
        TaggedBase64::new(PRIVKEY_ID, self.priv_key.as_ref()).unwrap()
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

impl fmt::Display for Ed25519Priv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let base64 = self.to_tagged_base64();
        write!(f, "{}", tagged_base64::to_string(&base64))
    }
}

impl Serialize for Ed25519Priv {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Ed25519Priv {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let base64 = String::deserialize(deserializer)?;
        Self::from_str(&base64).map_err(D::Error::custom)
    }
}

impl FromStr for Ed25519Priv {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let base64 =
            TaggedBase64::from_str(s).map_err(|e| format!("Could not decode Ed25519Pub: {e:?}"))?;
        if base64.tag() != PRIVKEY_ID {
            return Err(format!(
                "Invalid Ed25519Priv tag: {:?}, expected {:?}",
                base64.tag(),
                PRIVKEY_ID
            ));
        }

        match Self::from_bytes(&base64.value()) {
            Some(key) => Ok(key),
            None => Err("Failed to decode Ed25519 key".to_string()),
        }
    }
}
