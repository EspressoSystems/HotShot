//! This file contains the `FileBackedStore` struct, which is a wrapper around a `RecordStore`
//! that occasionally saves the DHT to a file on disk.

use std::collections::HashMap;

use anyhow::Context;
use delegate::delegate;
use libp2p::kad::store::{RecordStore, Result};
use tracing::{debug, warn};

/// A `RecordStore` wrapper that occasionally saves the DHT to a file on disk.
pub struct FileBackedStore<R: RecordStore> {
    /// The underlying store
    underlying_store: R,

    /// The path to the file
    path: String,

    /// The running delta between the records in the file and the records in the underlying store
    record_delta: u64,
}

impl<R: RecordStore> FileBackedStore<R> {
    /// Create a new `FileBackedStore` with the given underlying store and path
    pub fn new(underlying_store: R, path: String) -> Self {
        // Create the new store
        let mut store = FileBackedStore {
            underlying_store,
            path: path.clone(),
            record_delta: 0,
        };

        // Try to restore the DHT from a file. If it fails, warn and start with an empty store
        if let Err(err) = store.restore_from_file(path) {
            warn!(
                "Failed to restore DHT from file: {:?}. Starting with empty store",
                err
            );
        }

        // Return the new store
        store
    }

    /// Attempt to restore the DHT to the underlying store from the file at the given path
    pub fn restore_from_file(&mut self, path: String) -> anyhow::Result<()> {
        // Read the contents of the file as a `HashMap` of `Key` to `Vec<u8>`
        let contents = std::fs::read_to_string(path).with_context(|| "Failed to read DHT file")?;

        // Convert the contents to a `HashMap` of `RecordKey` to `Vec<u8>`
        let map: HashMap<libp2p::kad::RecordKey, Vec<u8>> =
            serde_json::from_str(&contents).with_context(|| "Failed to parse DHT file")?;

        // Get all records from the original store and put them in the new store
        for (record_key, record_value) in map {
            if let Err(err) = self
                .underlying_store
                .put(libp2p::kad::Record::new(record_key, record_value))
            {
                warn!("Failed to restore record: {:?}", err);
            }
        }

        Ok(())
    }

    /// Attempt to save the DHT to the file at the given path
    pub fn save_to_file(&mut self) -> anyhow::Result<()> {
        debug!("Saving DHT to file");

        // Get all records from the underlying store
        let records = self.underlying_store.records();

        // Convert the records to a `HashMap` of `RecordKey` to `Vec<u8>`
        let map: HashMap<libp2p::kad::RecordKey, Vec<u8>> =
            records.map(|r| (r.key.clone(), r.value.clone())).collect();

        // Serialize the map to a JSON string
        let contents = serde_json::to_string(&map).with_context(|| "Failed to serialize DHT")?;

        // Write the contents to the file
        std::fs::write(self.path.clone(), contents)
            .with_context(|| "Failed to write DHT to file")?;

        debug!("Saved DHT to file");

        Ok(())
    }
}

/// Implement the `RecordStore` trait for `FileBackedStore`
impl<R: RecordStore> RecordStore for FileBackedStore<R> {
    type ProvidedIter<'a>
        = R::ProvidedIter<'a>
    where
        R: 'a;
    type RecordsIter<'a>
        = R::RecordsIter<'a>
    where
        R: 'a;

    // Delegate all `RecordStore` methods except `put` to the inner store
    delegate! {
        to self.underlying_store {
            fn add_provider(&mut self, record: libp2p::kad::ProviderRecord) -> libp2p::kad::store::Result<()>;
            fn get(&self, k: &libp2p::kad::RecordKey) -> Option<std::borrow::Cow<'_, libp2p::kad::Record>>;
            fn provided(&self) -> Self::ProvidedIter<'_>;
            fn providers(&self, key: &libp2p::kad::RecordKey) -> Vec<libp2p::kad::ProviderRecord>;
            fn records(&self) -> Self::RecordsIter<'_>;
            fn remove_provider(&mut self, k: &libp2p::kad::RecordKey, p: &libp2p::PeerId);
        }
    }

    /// Overwrite the `put` method to potentially save the record to a file
    fn put(&mut self, record: libp2p::kad::Record) -> Result<()> {
        // Try to write to the underlying store
        let result = self.underlying_store.put(record);

        // If the record was successfully written, update the record delta
        if result.is_ok() {
            self.record_delta += 1;

            // If the record delta is greater than 10, try to save the file
            if self.record_delta > 10 {
                if let Err(e) = self.save_to_file() {
                    warn!("Failed to save DHT to file: {:?}", e);
                }
            }
        }

        result
    }

    /// Overwrite the `remove` method to potentially remove the record from a file
    fn remove(&mut self, k: &libp2p::kad::RecordKey) {
        // Remove the record from the underlying store
        self.underlying_store.remove(k);

        // Update the record delta
        self.record_delta += 1;

        // If the record delta is greater than 10, try to save the file
        if self.record_delta > 10 {
            if let Err(e) = self.save_to_file() {
                warn!("Failed to save DHT to file: {:?}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use libp2p::{
        kad::{store::MemoryStore, RecordKey},
        PeerId,
    };

    use super::*;

    #[test]
    fn test_save_and_restore() {
        // Create a test store
        let mut store = FileBackedStore::new(
            MemoryStore::new(PeerId::random()),
            "/tmp/test.dht".to_string(),
        );

        // The key is a string
        let key = RecordKey::new(&b"test_key".to_vec());

        // The value is a random 16-byte array
        let random_value = rand::random::<[u8; 16]>();

        // Put a record into the store
        store
            .put(libp2p::kad::Record::new(key.clone(), random_value.to_vec()))
            .expect("Failed to put record into store");

        // Save the store to a file
        store.save_to_file().expect("Failed to save store to file");

        // Create a new store from the file
        let new_store = FileBackedStore::new(
            MemoryStore::new(PeerId::random()),
            "/tmp/test.dht".to_string(),
        );

        // Check that the new store has the record
        let restored_record = new_store
            .get(&key)
            .expect("Failed to get record from store");

        // Check that the restored record has the same value as the original record
        assert_eq!(restored_record.value, random_value.to_vec());
    }
}
