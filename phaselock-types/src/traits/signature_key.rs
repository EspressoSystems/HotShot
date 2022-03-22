//! Minimal abstraction over public key signatures
use threshold_crypto::SignatureShare;

use crate::{PrivKey, PubKey};

/// Trait for abstracting public key signatures
pub trait SignatureKey {
    /// The private key type for this signature algorithm
    type PrivateKey;
    // Signature type represented as a vec/slice of bytes to let the implementer handle the nuances
    // of serialization, to avoid Cryptographic pitfalls
    /// Validate a signature
    fn validate(&self, signature: &[u8], data: &[u8]) -> bool;
    /// Produce a signature
    fn sign(private_key: &Self::PrivateKey, data: &[u8]) -> Vec<u8>;
}

/// Implementation for [`PubKey`] type that works on partial signatures
impl SignatureKey for PubKey {
    type PrivateKey = PrivKey;

    fn validate(&self, signature: &[u8], data: &[u8]) -> bool {
        // make sure the signature is the right length, 96 bytes for this algorithm
        if signature.len() != 96 {
            return false;
        }
        #[cfg(not(feature = "test_crypto"))]
        let mut share_slice = [0; 96];
        #[cfg(feature = "test_crypto")]
        let mut share_slice = [0; 4];
        share_slice.copy_from_slice(signature);
        if let Ok(signature_share) = SignatureShare::from_bytes(&share_slice) {
            self.node.verify(&signature_share, data)
        } else {
            false
        }
    }

    fn sign(private_key: &Self::PrivateKey, data: &[u8]) -> Vec<u8> {
        private_key.node.sign(data).to_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use crate::{PrivKey, PubKey};

    use super::SignatureKey;

    #[test]
    fn tc_impl_smoke() {
        // get a secrete key share
        let sks = threshold_crypto::SecretKeySet::random(1, &mut rand::thread_rng());
        let private = sks.secret_key_share(12345_u64);
        let public = PubKey::from_secret_key_set_escape_hatch(&sks, 12345);
        let private = PrivKey { node: private };

        let mut data = [0_u8; 128];
        rand::thread_rng().fill(&mut data);

        // Make sure the signature validates when it should
        let mut signature = <PubKey as SignatureKey>::sign(&private, &data);
        assert!(public.validate(&signature, &data));
        // Corrupt the signature and make sure it no longer validates
        signature[0] = signature[0].wrapping_add(1);
        assert!(!public.validate(&signature, &data));
    }
}
