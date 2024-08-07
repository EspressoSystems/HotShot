use anyhow::{bail, Context, Result};
use hotshot_types::traits::signature_key::SignatureKey;
use libp2p::kad::Record;
use serde::{Deserialize, Serialize};
use tracing::warn;

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
#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum Namespace {
    /// A namespace for looking up P2P identities
    Lookup = 0,
}

/// Allow fallible conversion from a byte to a namespace
impl TryFrom<u8> for Namespace {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Lookup),
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
    pub fn validate(&self, record_key: &RecordKey) -> bool {
        if let Self::Signed(value, signature) = self {
            // If the request is "signed", the public key is the record's key
            let Ok(public_key) = K::from_bytes(record_key.key.as_slice()) else {
                warn!("Failed to deserialize signer's public key");
                return false;
            };

            // The value to sign should be the record key concatenated with the value
            let mut signed_value = record_key.to_bytes();
            signed_value.extend_from_slice(value);

            // Check the entire value
            public_key.validate(signature, &signed_value)
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
