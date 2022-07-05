//! Demonstration implementation of the [`SignatureKey`] trait using ed25519
use super::{EncodedPublicKey, EncodedSignature, SignatureKey, TestableSignatureKey};
use ed25519_compact::{KeyPair, Noise, PublicKey, SecretKey, Seed, Signature};
use espresso_systems_common::phaselock::PEER_ID;
use serde::{de::Error, Deserialize, Serialize};
use std::{cmp::Ordering, fmt, str::FromStr};
use tagged_base64::TaggedBase64;
use tracing::{debug, instrument, warn};

/// `Ed25519Priv` implementation
mod ed25519_priv;
/// `Ed25519Pub` implementation
mod ed25519_pub;

// impl Ed25519Pub {
//     /// Return the [`TaggedBase64`] representation of this key.
//     #[allow(clippy::missing_panics_doc)] // `TaggedBase64::new()` only panics if `PEER_ID` is not valid base64, which it is.
//     pub fn to_tagged_base64(&self) -> TaggedBase64 {
//         TaggedBase64::new(PEER_ID, self.pub_key.as_ref()).unwrap()
//     }
// }

// impl SignatureKey for Ed25519Pub {
//     type PrivateKey = Ed25519Priv;
//
//     #[instrument(skip(self))]
//     fn validate(&self, signature: &EncodedSignature, data: &[u8]) -> bool {
//         let signature = &signature.0[..];
//         // Convert to the signature type
//         match Signature::from_slice(signature) {
//             Ok(signature) => {
//                 // Validate the signature
//                 match self.pub_key.verify(data, &signature) {
//                     Ok(_) => true,
//                     Err(e) => {
//                         debug!(?e, "Signature failed verification");
//                         false
//                     }
//                 }
//             }
//             Err(e) => {
//                 // Log and error
//                 debug!(?e, "signature was structurally invalid");
//                 false
//             }
//         }
//     }
//
//     fn sign(private_key: &Self::PrivateKey, data: &[u8]) -> EncodedSignature {
//         // Generate some noise first
//         let noise = Noise::generate();
//         // Perform the signature
//         let signature = private_key.priv_key.sign(data, Some(noise));
//         // Convert the signature to bytes and return
//         EncodedSignature(signature.to_vec())
//     }
//
//     fn from_private(private_key: &Self::PrivateKey) -> Self {
//         let pub_key = private_key.priv_key.public_key();
//         Self { pub_key }
//     }
//
//     fn to_bytes(&self) -> EncodedPublicKey {
//         EncodedPublicKey(self.pub_key.to_vec())
//     }
//
//     #[instrument]
//     fn from_bytes(bytes: &EncodedPublicKey) -> Option<Self> {
//         let bytes = &bytes.0[..];
//         match PublicKey::from_slice(bytes) {
//             Ok(pub_key) => Some(Self { pub_key }),
//             Err(e) => {
//                 debug!(?e, "Failed to deserialize public key");
//                 None
//             }
//         }
//     }
// }

// impl TestableSignatureKey for Ed25519Pub {
//     fn generate_test_key(id: u64) -> Self::PrivateKey {
//         Ed25519Priv::generated_from_seed_indexed([0_u8; 32], id)
//     }
// }
//
// impl FromStr for Ed25519Pub {
//     type Err = String;
//
//     fn from_str(s: &str) -> Result<Self, String> {
//         let base64 = TaggedBase64::from_str(s)
//             .map_err(|e| format!("Could not decode Ed25519Pub: {:?}", e))?;
//         if base64.tag() != PEER_ID {
//             return Err(format!(
//                 "Invalid Ed25519Pub tag: {:?}, expected {:?}",
//                 base64.tag(),
//                 PEER_ID
//             ));
//         }
//
//         match Self::from_bytes(&EncodedPublicKey(base64.value())) {
//             Some(key) => Ok(key),
//             None => Err("Failed to decode Ed25519 key".to_string()),
//         }
//     }
// }
//
// impl fmt::Display for Ed25519Pub {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         let base64 = self.to_tagged_base64();
//         write!(f, "{}", tagged_base64::to_string(&base64))
//     }
// }
//
// impl Serialize for Ed25519Pub {
//     fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
//     where
//         S: serde::Serializer,
//     {
//         serializer.serialize_str(&self.to_string())
//     }
// }
//
// impl<'de> Deserialize<'de> for Ed25519Pub {
//     fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
//     where
//         D: serde::Deserializer<'de>,
//     {
//         let base64 = String::deserialize(deserializer)?;
//         Self::from_str(&base64).map_err(D::Error::custom)
//     }
// }
pub use self::ed25519_priv::Ed25519Priv;
pub use self::ed25519_pub::Ed25519Pub;

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
        assert_eq!(pub_key, pub_key_2);

        // Serialize the public key and back, then verify equality
        let serialized = serde_json::to_string(&pub_key).expect("Failed to ser key");
        let pub_key_2: Ed25519Pub = serde_json::from_str(&serialized).expect("Failed to deser key");
        assert_eq!(pub_key, pub_key_2);

        // .to_string() and FromStr
        let str = pub_key.to_string();
        let pub_key_2: Ed25519Pub = str.parse().expect("Failed to parse key");
        assert_eq!(pub_key, pub_key_2);
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
