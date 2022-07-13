//! Minimal abstraction over public key signatures
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{fmt::Debug, hash::Hash};

#[cfg(feature = "demo")]
pub mod ed25519;

/// Type saftey wrapper for byte encoded keys
#[derive(
    Clone, custom_debug::Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
pub struct EncodedPublicKey(#[debug(with = "custom_debug::hexbuf")] pub Vec<u8>);

/// Type saftey wrapper for byte encoded signature
#[derive(
    Clone, custom_debug::Debug, Hash, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
pub struct EncodedSignature(#[debug(with = "custom_debug::hexbuf")] pub Vec<u8>);

/// Trait for abstracting public key signatures
pub trait SignatureKey:
    Send + Sync + Clone + Sized + Debug + Hash + Serialize + DeserializeOwned + PartialEq + Eq + Unpin
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
}

/// Trait for generation of keys during testing
pub trait TestableSignatureKey: SignatureKey {
    /// Generates a private key from the given integer seed
    fn generate_test_key(id: u64) -> Self::PrivateKey;
}
