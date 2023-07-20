use super::{Ed25519Priv, EncodedPublicKey, EncodedSignature, SignatureKey, TestableSignatureKey};
use ed25519_compact::{PublicKey};
use espresso_systems_common::hotshot::tag::PEER_ID;
use serde::{de::Error, Deserialize, Serialize};
use std::{cmp::Ordering, fmt, str::FromStr};
use tagged_base64::TaggedBase64;
use tracing::{debug, instrument, warn};
use hotshot_primitives::quorum_certificate::{BitvectorQuorumCertificate, QuorumCertificateValidation};
use jf_primitives::signatures::bls_over_bn254::{BLSOverBN254CurveSignatureScheme, KeyPair as QCKeyPair, VerKey};
use jf_primitives::signatures::SignatureScheme;
use blake3::traits::digest::generic_array::GenericArray;
use typenum::U32;
use bincode::Options;
use hotshot_utils::bincode::bincode_opts;
/// Public key type for an ed25519 [`SignatureKey`] pair
///
/// This type makes use of noise for non-determinisitc signatures.
#[derive(Clone, PartialEq, Eq, Hash, Copy)]


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
    #[must_use]
    pub fn to_tagged_base64(&self) -> TaggedBase64 {
        TaggedBase64::new(PEER_ID, self.pub_key.as_ref()).unwrap()
    }
}

impl SignatureKey for Ed25519Pub {
    type PrivateKey = Ed25519Priv;

    #[instrument(skip(self))]
    fn validate(&self, ver_key: VerKey, signature: &EncodedSignature, data: &[u8]) -> bool {
        let x: Result<<BLSOverBN254CurveSignatureScheme as SignatureScheme>::Signature, _> = 
            bincode_opts().deserialize(&signature.0);
            match x {
                Ok(s) => {
                    //Sishan Note: This is the validation for QC partial signature before append().
                    let generic_msg: &GenericArray<u8, U32> = GenericArray::from_slice(data);
                    BLSOverBN254CurveSignatureScheme::verify(
                        &(),
                        &ver_key, 
                        &generic_msg,
                        &s,
                    ).is_ok()
                }
                Err(_) => false,
            }
    }

    fn sign(key_pair: QCKeyPair, data: &[u8]) -> EncodedSignature {
        let generic_msg = GenericArray::from_slice(data);
        let agg_signature_test = BitvectorQuorumCertificate::<BLSOverBN254CurveSignatureScheme>::partial_sign(
            &(),
            // &msg_test.into(),
            &generic_msg,
            key_pair.sign_key_ref(),
            &mut rand::thread_rng(),
        ).unwrap();
        // Convert the signature to bytes and return
        let bytes = bincode_opts()
            .serialize(&agg_signature_test)
            .expect("This serialization shouldn't be able to fail");
        // let print_bytes = String::from_utf8_lossy(&bytes);
        EncodedSignature(bytes)
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

    fn generated_from_seed_indexed(seed: [u8; 32], index: u64) -> (Self, Self::PrivateKey) {
        let priv_key = Self::PrivateKey::generated_from_seed_indexed(seed, index);
        (Self::from_private(&priv_key), priv_key)
    }
}

impl TestableSignatureKey for Ed25519Pub {
    fn generate_test_key(id: u64) -> Self::PrivateKey {
        Ed25519Priv::generated_from_seed_indexed([0_u8; 32], id)
    }
}

impl FromStr for Ed25519Pub {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let base64 =
            TaggedBase64::from_str(s).map_err(|e| format!("Could not decode Ed25519Pub: {e:?}"))?;
        if base64.tag() != PEER_ID {
            return Err(format!(
                "Invalid Ed25519Pub tag: {:?}, expected {:?}",
                base64.tag(),
                PEER_ID
            ));
        }

        match Self::from_bytes(&EncodedPublicKey(base64.value())) {
            Some(key) => Ok(key),
            None => Err("Failed to decode Ed25519 key".to_string()),
        }
    }
}

impl fmt::Display for Ed25519Pub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let base64 = self.to_tagged_base64();
        write!(f, "{}", tagged_base64::to_string(&base64))
    }
}

impl Serialize for Ed25519Pub {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Ed25519Pub {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let base64 = String::deserialize(deserializer)?;
        Self::from_str(&base64).map_err(D::Error::custom)
    }
}
