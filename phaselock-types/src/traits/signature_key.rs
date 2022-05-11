//! Minimal abstraction over public key signatures
use std::{fmt::Debug, hash::Hash};

use serde::{de::DeserializeOwned, Serialize};

#[cfg(feature = "demo")]
pub mod ed25519;

/// Trait for abstracting public key signatures
pub trait SignatureKey:
    Send + Sync + Clone + Sized + Debug + Hash + Serialize + DeserializeOwned + PartialEq + Eq + Unpin
{
    /// The private key type for this signature algorithm
    type PrivateKey: Send + Sync + Sized;
    // Signature type represented as a vec/slice of bytes to let the implementer handle the nuances
    // of serialization, to avoid Cryptographic pitfalls
    /// Validate a signature
    fn validate(&self, signature: &[u8], data: &[u8]) -> bool;
    /// Produce a signature
    fn sign(private_key: &Self::PrivateKey, data: &[u8]) -> Vec<u8>;
    /// Produce a public key from a private key
    fn from_private(private_key: &Self::PrivateKey) -> Self;
    /// Serialize a public key to bytes
    fn to_bytes(&self) -> Vec<u8>;
    /// Deserialize a public key from bytes
    fn from_bytes(bytes: &[u8]) -> Option<Self>;
}
