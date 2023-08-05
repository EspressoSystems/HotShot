//! Demonstration implementation of the [`SignatureKey`] trait using ed25519
use super::{EncodedPublicKey, EncodedSignature, SignatureKey};
/// `Ed25519Priv` implementation
mod ed25519_priv;
/// `Ed25519Pub` implementation
mod ed25519_pub;

pub use self::{ed25519_priv::Ed25519Priv, ed25519_pub::Ed25519Pub};
use jf_primitives::signatures::{bls_over_bn254::VerKey};
use jf_primitives::signatures::bls_over_bn254::{BLSOverBN254CurveSignatureScheme, KeyPair as QCKeyPair};

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

        // KeyPair with signature scheme for certificate Aggregation
        let key_pair = QCKeyPair::generate(&mut rand::thread_rng());

        // Sign the data with it
        let signature = Ed25519Pub::sign(key_pair.clone(), &data);
        // Verify the signature
        assert!(pub_key.validate(key_pair.ver_key(), &signature, &data));
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
