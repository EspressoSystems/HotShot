//! Types and Traits for the `PhaseLock` consensus module
#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::panic
)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

pub mod constants;
pub mod data;
pub mod error;
pub mod event;
pub mod message;
pub mod traits;

use data::{create_verify_hash, LeafHash, Stage, ViewNumber};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use traits::signature_key::SignatureKey;

/// Public key type
///
/// Opaque wrapper around `threshold_crypto` key
///
/// TODO: These items don't really need to be pub, but are due to the pain in the ass factor of the
/// migration to a split crate
#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, custom_debug::Debug, Clone)]
pub struct PubKey<S> {
    /// The portion of the KeyShare this node holds
    pub nonce: u64,
    /// The public key of this node
    #[debug(skip)]
    pub key: S,
}

impl<S: SignatureKey> PubKey<S> {
    /// Create a new pubkey, which currently only holdes the node's id
    #[allow(dead_code)]
    pub fn new(nonce: u64, key: S) -> PubKey<S> {
        PubKey { nonce, key }
    }
}

impl<S: Eq> PartialOrd for PubKey<S> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.nonce.partial_cmp(&other.nonce)
    }
}

impl<S: Eq> Ord for PubKey<S> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.nonce.cmp(&other.nonce)
    }
}
