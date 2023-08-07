use super::{BN254Priv, EncodedPublicKey, EncodedSignature, SignatureKey};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    fmt::Debug,
    str::FromStr,
};
use tracing::{debug, instrument, warn};
use hotshot_primitives::qc::bit_vector::BitVectorQC;
use jf_primitives::signatures::bls_over_bn254::{BLSOverBN254CurveSignatureScheme, KeyPair as QCKeyPair, VerKey};
use hotshot_primitives::qc::QuorumCertificate as AssembledQuorumCertificate;
use jf_primitives::signatures::SignatureScheme;
use blake3::traits::digest::generic_array::GenericArray;
use typenum::U32;
use bincode::Options;
use hotshot_utils::bincode::bincode_opts;

/// Public key type for an bn254 [`SignatureKey`] pair
///
/// This type makes use of noise for non-determinisitc signatures.
#[derive(Clone, PartialEq, Eq, Hash, Copy, Serialize, Deserialize, Debug)]


pub struct BN254Pub {
    /// The public key for this keypair
    pub_key: VerKey,
}


impl PartialOrd for BN254Pub {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let self_bytes = &self.pub_key.to_string();
        let other_bytes = &other.pub_key.to_string();
        self_bytes.partial_cmp(other_bytes)
    }
}

impl Ord for BN254Pub {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_bytes = &self.pub_key.to_string();
        let other_bytes = &other.pub_key.to_string();
        self_bytes.cmp(other_bytes)
    }
}


impl SignatureKey for BN254Pub {
    type PrivateKey = BN254Priv;

    #[instrument(skip(self))]
    fn validate(&self, ver_key: VerKey, signature: &EncodedSignature, data: &[u8]) -> bool {
        let x: Result<<BLSOverBN254CurveSignatureScheme as SignatureScheme>::Signature, _> = 
            bincode_opts().deserialize(&signature.0);
            match x {
                Ok(s) => {
                    // This is the validation for QC partial signature before append().
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
        let agg_signature_test_wrap = BitVectorQC::<BLSOverBN254CurveSignatureScheme>::sign(
            &(),
            &generic_msg,
            key_pair.sign_key_ref(),
            &mut rand::thread_rng(),
        );
        match agg_signature_test_wrap {
            Ok(agg_signature_test) => {
                // Convert the signature to bytes and return
                let bytes = bincode_opts()
                .serialize(&agg_signature_test);
                match bytes {
                    Ok(bytes) => {EncodedSignature(bytes)}
                    Err(e) => {
                        warn!(?e, "Failed to serialize signature in sign()");
                        EncodedSignature(vec![])
                    }
                }
            }
            Err(e) => {
                warn!(?e, "Failed to sign");
                EncodedSignature(vec![])
            }
        }
    }

    fn from_private(private_key: &Self::PrivateKey) -> Self {
        let pub_key = VerKey::from(&private_key.priv_key);
        Self { pub_key }
    }

    fn to_bytes(&self) -> EncodedPublicKey {
        let pub_key_bytes = bincode_opts()
            .serialize(&self.pub_key)
            .expect("This serialization shouldn't be able to fail");
        EncodedPublicKey(pub_key_bytes)
    }

    #[instrument]
    fn from_bytes(bytes: &EncodedPublicKey) -> Option<Self> {
        let x: Result<VerKey, _> = 
            bincode_opts().deserialize(&bytes.0);
        match x {
            Ok(pub_key) => {
                Some(BN254Pub{ pub_key: pub_key })
            }
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
