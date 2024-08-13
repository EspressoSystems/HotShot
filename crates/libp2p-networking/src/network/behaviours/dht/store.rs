//! This file contains the `ValidatedStore` struct, which is a wrapper around a `RecordStore` that
//! validates records before storing them.
//!
//! The `ValidatedStore` struct is used to ensure that only valid records are stored in the DHT.

use std::marker::PhantomData;

use delegate::delegate;
use hotshot_types::traits::signature_key::SignatureKey;
use libp2p::kad::store::{Error, RecordStore, Result};
use tracing::warn;

use crate::network::behaviours::dht::record::RecordKey;

use super::record::RecordValue;

/// A `RecordStore` wrapper that validates records before storing them.
pub struct ValidatedStore<R: RecordStore, K: SignatureKey> {
    /// The underlying store
    store: R,

    /// Phantom type for the key
    phantom: std::marker::PhantomData<K>,
}

impl<R: RecordStore, K: SignatureKey> ValidatedStore<R, K> {
    /// Create a new `ValidatedStore` with the given underlying store
    pub fn new(store: R) -> Self {
        ValidatedStore {
            store,
            phantom: PhantomData,
        }
    }
}

/// Implement the `RecordStore` trait for `ValidatedStore`
impl<R: RecordStore, K: SignatureKey> RecordStore for ValidatedStore<R, K>
where
    K: 'static,
{
    type ProvidedIter<'a> = R::ProvidedIter<'a> where R: 'a, K: 'a;
    type RecordsIter<'a> = R::RecordsIter<'a> where R: 'a, K: 'a;

    // Delegate all `RecordStore` methods except `put` to the inner store
    delegate! {
        to self.store{
            fn add_provider(&mut self, record: libp2p::kad::ProviderRecord) -> libp2p::kad::store::Result<()>;
            fn get(&self, k: &libp2p::kad::RecordKey) -> Option<std::borrow::Cow<'_, libp2p::kad::Record>>;
            fn provided(&self) -> Self::ProvidedIter<'_>;
            fn providers(&self, key: &libp2p::kad::RecordKey) -> Vec<libp2p::kad::ProviderRecord>;
            fn records(&self) -> Self::RecordsIter<'_>;
            fn remove(&mut self, k: &libp2p::kad::RecordKey);
            fn remove_provider(&mut self, k: &libp2p::kad::RecordKey, p: &libp2p::PeerId);
        }
    }

    /// Overwrite the `put` method to validate the record before storing it
    fn put(&mut self, record: libp2p::kad::Record) -> Result<()> {
        // Convert the record to the correct type
        if let Ok(record_value) = RecordValue::<K>::try_from(record.clone()) {
            // Convert the key to the correct type
            let Ok(record_key) = RecordKey::try_from_bytes(&record.key.to_vec()) else {
                warn!("Failed to convert record key");
                return Err(Error::ValueTooLarge);
            };

            // If the record is signed by the correct key,
            if record_value.validate(&record_key) {
                // Store the record
                if let Err(err) = self.store.put(record.clone()) {
                    warn!("Failed to store record: {:?}", err);
                    return Err(Error::ValueTooLarge);
                }
            } else {
                warn!("Failed to validate record");
                return Err(Error::ValueTooLarge);
            }
        }

        Ok(())
    }
}
