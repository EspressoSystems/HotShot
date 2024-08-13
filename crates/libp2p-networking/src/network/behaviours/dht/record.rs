use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};

use anyhow::{bail, Context, Result};
use blake3::{Hash, Hasher};
use hotshot_types::traits::signature_key::SignatureKey;
use libp2p::kad::Record;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use tracing::{error, warn};

/// A cache for record validation results
pub type RecordValidationCache = Arc<Mutex<LruCache<Hash, bool>>>;

/// Create and return a new record validation cache
/// with a default size of 100
///
/// # Panics
/// If the cache size is zero
#[must_use]
pub fn new_record_validation_cache() -> RecordValidationCache {
    Arc::new(Mutex::new(LruCache::new(
        NonZeroUsize::new(100).expect("Cache size is zero"),
    )))
}

/// A (signed or unsigned) record value to be stored (serialized) in the DHT.
/// This is a wrapper around a value that includes a possible signature.
#[derive(Serialize, Deserialize, Clone)]
pub enum RecordValue<K: SignatureKey + 'static> {
    /// A signed record value
    Signed(Vec<u8>, K::PureAssembledSignatureType),

    /// An unsigned record value
    Unsigned(Vec<u8>),
}

/// The namespace of a record. This is included with the key
/// and allows for multiple types of records to be stored in the DHT.
#[repr(u8)]
#[derive(Serialize, Deserialize, Clone, Copy, Eq, PartialEq)]
pub enum Namespace {
    /// A namespace for looking up P2P identities
    Lookup = 0,

    #[cfg(test)]
    /// A secondary record type for testing purposes
    Testing = u8::MAX,
}

/// Allow fallible conversion from a byte to a namespace
impl TryFrom<u8> for Namespace {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Lookup),
            #[cfg(test)]
            1 => Ok(Self::Testing),
            _ => bail!("Unknown namespace"),
        }
    }
}

/// A record's key. This is a concatenation of the namespace and the key.
#[derive(Clone)]
pub struct RecordKey {
    /// The namespace of the record key
    pub namespace: Namespace,

    /// The actual key
    pub key: Vec<u8>,
}

impl RecordKey {
    #[must_use]
    /// Create and return a new record key in the given namespace
    pub fn new(namespace: Namespace, key: Vec<u8>) -> Self {
        Self { namespace, key }
    }

    /// Convert the record key to a byte vector
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        // Concatenate the namespace and the key
        let mut bytes = vec![self.namespace as u8];
        bytes.extend_from_slice(&self.key);
        bytes
    }

    /// Try to convert a byte vector to a record key
    ///
    /// # Errors
    /// If the provided array is empty
    pub fn try_from_bytes(bytes: &[u8]) -> Result<Self> {
        // Check if the bytes are empty
        if bytes.is_empty() {
            bail!("Empty record key bytes")
        }

        // The first byte is the namespace
        let namespace = Namespace::try_from(bytes[0])?;

        // Return the record key
        Ok(Self {
            namespace,
            key: bytes[1..].to_vec(),
        })
    }
}

impl<K: SignatureKey + 'static> RecordValue<K> {
    /// Creates and returns a new signed record by signing the key and value
    /// with the private key
    ///
    /// # Errors
    /// - If we fail to sign the value
    /// - If we fail to serialize the signature
    pub fn new_signed(
        record_key: &RecordKey,
        value: Vec<u8>,
        private_key: &K::PrivateKey,
    ) -> Result<Self> {
        // The value to sign should be the record key concatenated with the value
        let mut value_to_sign = record_key.to_bytes();
        value_to_sign.extend_from_slice(&value);

        let signature =
            K::sign(private_key, &value_to_sign).with_context(|| "Failed to sign record")?;

        // Return the signed record
        Ok(Self::Signed(value, signature))
    }

    /// Creates and returns a new unsigned record
    #[must_use]
    pub fn new(value: Vec<u8>) -> Self {
        Self::Unsigned(value)
    }

    /// If the message requires authentication, validate the record by verifying the signature with the
    /// given key
    pub fn validate(&self, record_key: &RecordKey, cache: &RecordValidationCache) -> bool {
        if let Self::Signed(value, signature) = self {
            // If the request is "signed", the public key is the record's key
            let Ok(public_key) = K::from_bytes(record_key.key.as_slice()) else {
                warn!("Failed to deserialize signer's public key");
                return false;
            };

            // The value to sign should be the record key concatenated with the value
            let mut signed_value = record_key.to_bytes();
            signed_value.extend_from_slice(value);

            // Serialize signature
            let Ok(signature_bytes) = bincode::serialize(signature) else {
                warn!("Failed to serialize signature");
                return false;
            };

            // Hash the public key, signature, and signed value
            let hash = Hasher::new()
                .update(record_key.key.as_slice())
                .update(&signature_bytes)
                .update(&signed_value)
                .finalize();

            // Try to lock the cache
            let Ok(mut cache_guard) = cache.lock() else {
                error!("Failed to lock record validation cache");
                return false;
            };

            // Check if we have already validated this signature
            if let Some(valid) = cache_guard.get(&hash) {
                return *valid;
            }
            drop(cache_guard);

            // Check the value
            let result = public_key.validate(signature, &signed_value);

            // Cache the result
            let Ok(mut cache_guard) = cache.lock() else {
                error!("Failed to lock record validation cache");
                return false;
            };
            cache_guard.put(hash, result);
            drop(cache_guard);

            result
        } else {
            true
        }
    }

    /// Get the underlying value of the record
    pub fn value(&self) -> &[u8] {
        match self {
            Self::Unsigned(value) | Self::Signed(value, _) => value,
        }
    }
}

impl<K: SignatureKey + 'static> TryFrom<Record> for RecordValue<K> {
    type Error = anyhow::Error;

    fn try_from(record: Record) -> Result<Self> {
        // Deserialize the record value
        let record: RecordValue<K> = bincode::deserialize(&record.value)
            .with_context(|| "Failed to deserialize record value")?;

        // Return the record
        Ok(record)
    }
}

#[cfg(test)]
mod test {
    use hotshot_types::signature_key::BLSPubKey;

    use super::*;

    /// Test that namespace serialization and deserialization is consistent
    #[test]
    fn test_namespace_serialization_parity() {
        // Serialize the namespace
        let namespace = Namespace::Lookup;
        let bytes = namespace as u8;

        // Deserialize the namespace
        let namespace = Namespace::try_from(bytes).expect("Failed to deserialize namespace");
        assert!(namespace == Namespace::Lookup, "Wrong namespace");
    }

    /// Test that record key serialization and deserialization is consistent
    #[test]
    fn test_record_key_serialization_parity() {
        // Create a new record key
        let namespace = Namespace::Lookup;
        let key = vec![1, 2, 3, 4];
        let record_key = RecordKey::new(namespace, key.clone());

        // Serialize it
        let bytes = record_key.to_bytes();

        // Deserialize it
        let record_key =
            RecordKey::try_from_bytes(&bytes).expect("Failed to deserialize record key");

        // Make sure the deserialized record key is the same as the original
        assert!(record_key.namespace == namespace, "Namespace mismatch");
        assert!(record_key.key == key, "Key mismatch");
    }

    /// Test that the validity of a valid, signed record is correct
    #[test]
    fn test_valid_signature() {
        // Generate a staking keypair
        let (public_key, private_key) = BLSPubKey::generated_from_seed_indexed([1; 32], 1337);

        // Create a value. The key is the public key
        let value = vec![5, 6, 7, 8];

        // Create a record key (as we need to sign both the key and the value)
        let record_key = RecordKey::new(Namespace::Lookup, public_key.to_bytes());

        // Sign the record and value with the private key
        let record_value: RecordValue<BLSPubKey> =
            RecordValue::new_signed(&record_key, value.clone(), &private_key).unwrap();

        // Validate the signed record
        assert!(
            record_value.validate(&record_key, &new_record_validation_cache(),),
            "Failed to validate signed record"
        );
    }

    /// Test that altering the namespace byte causes a validation failure
    #[test]
    fn test_invalid_namespace() {
        // Generate a staking keypair
        let (public_key, private_key) = BLSPubKey::generated_from_seed_indexed([1; 32], 1337);

        // Create a value. The key is the public key
        let value = vec![5, 6, 7, 8];

        // Create a record key (as we need to sign both the key and the value)
        let mut record_key = RecordKey::new(Namespace::Lookup, public_key.to_bytes());

        // Sign the record and value with the private key
        let record_value: RecordValue<BLSPubKey> =
            RecordValue::new_signed(&record_key, value.clone(), &private_key).unwrap();

        // Alter the namespace
        record_key.namespace = Namespace::Testing;

        // Validate the signed record
        assert!(
            !record_value.validate(&record_key, &new_record_validation_cache(),),
            "Failed to detect invalid namespace"
        );
    }

    /// Test that altering the contents of the record key causes a validation failure
    #[test]
    fn test_invalid_key() {
        // Generate a staking keypair
        let (public_key, private_key) = BLSPubKey::generated_from_seed_indexed([1; 32], 1337);

        // Create a value. The key is the public key
        let value = vec![5, 6, 7, 8];

        // Create a record key (as we need to sign both the key and the value)
        let mut record_key = RecordKey::new(Namespace::Lookup, public_key.to_bytes());

        // Sign the record and value with the private key
        let record_value: RecordValue<BLSPubKey> =
            RecordValue::new_signed(&record_key, value.clone(), &private_key).unwrap();

        // Set the key to a different one
        record_key.key = BLSPubKey::generated_from_seed_indexed([1; 32], 1338)
            .0
            .to_bytes();

        // Validate the signed record
        assert!(
            !record_value.validate(&record_key, &new_record_validation_cache(),),
            "Failed to detect invalid record key"
        );
    }

    /// Test that unsigned records are always valid
    #[test]
    fn test_unsigned_record_is_valid() {
        // Create a value
        let value = vec![5, 6, 7, 8];

        // Create a record key
        let record_key = RecordKey::new(Namespace::Lookup, vec![1, 2, 3, 4]);

        // Create an unsigned record
        let record_value: RecordValue<BLSPubKey> = RecordValue::new(value.clone());

        // Validate the unsigned record
        assert!(
            record_value.validate(&record_key, &new_record_validation_cache(),),
            "Failed to validate unsigned record"
        );
    }
}