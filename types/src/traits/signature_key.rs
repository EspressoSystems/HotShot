//! Minimal compatibility over public key signatures
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize, Read, SerializationError, Write};
use espresso_systems_common::hotshot::tag;
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, hash::Hash};
use tagged_base64::tagged;

#[cfg(feature = "demo")]
pub mod ed25519;

/// Type saftey wrapper for byte encoded keys
#[tagged(tag::ENCODED_PUB_KEY)]
#[derive(
    Clone,
    custom_debug::Debug,
    Hash,
    CanonicalSerialize,
    CanonicalDeserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
)]
pub struct EncodedPublicKey(#[debug(with = "custom_debug::hexbuf")] pub Vec<u8>);

/// Type saftey wrapper for byte encoded signature
#[derive(
    Clone, custom_debug::Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
pub struct EncodedSignature(#[debug(with = "custom_debug::hexbuf")] pub Vec<u8>);

impl AsRef<[u8]> for EncodedSignature {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

/// Trait for abstracting public key signatures
pub trait SignatureKey:
    Send + Sync + Clone + Sized + Debug + Hash + Serialize + for<'a> Deserialize<'a> + PartialEq + Eq
{
    /// The private key type for this signature algorithm
    type PrivateKey: Send + Sync + Sized;
    // Signature type represented as a vec/slice of bytes to let the implementer handle the nuances
    // of serialization, to avoid Cryptographic pitfalls
    /// Validate a signature
    fn validate(&self, signature: &EncodedSignature, data: &[u8]) -> bool;
    /// Produce a signature
    fn sign(private_key: &Self::PrivateKey, data: &[u8]) -> EncodedSignature;
    /// Produce a public key from a private key
    fn from_private(private_key: &Self::PrivateKey) -> Self;
    /// Serialize a public key to bytes
    fn to_bytes(&self) -> EncodedPublicKey;
    /// Deserialize a public key from bytes
    fn from_bytes(bytes: &EncodedPublicKey) -> Option<Self>;

    /// Generate a new key pair
    fn generated_from_seed_indexed(seed: [u8; 32], index: u64) -> (Self, Self::PrivateKey);
}

/// TODO we don't need this if we have `generated_from_seed_indexed`
/// should use one or the other
/// Trait for generation of keys during testing
pub trait TestableSignatureKey: SignatureKey {
    /// Generates a private key from the given integer seed
    fn generate_test_key(id: u64) -> Self::PrivateKey;
}
