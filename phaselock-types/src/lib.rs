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
pub use threshold_crypto as tc;

pub mod constants;
pub mod data;
pub mod error;
pub mod event;
pub mod message;
pub mod traits;

use serde::{Deserialize, Serialize};

use std::fmt::Debug;

use data::{create_verify_hash, LeafHash, Stage, ViewNumber};

/// Public key type
///
/// Opaque wrapper around `threshold_crypto` key
///
/// TODO: These items don't really need to be pub, but are due to the pain in the ass factor of the
/// migration to a split crate
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, custom_debug::Debug)]
pub struct PubKey {
    /// Overall public key set for the network
    #[debug(skip)]
    pub set: tc::PublicKeySet,
    /// The public key share that this node holds
    #[debug(skip)]
    pub node: tc::PublicKeyShare,
    /// The portion of the KeyShare this node holds
    pub nonce: u64,
}

impl PubKey {
    /// Testing only random key generation
    #[allow(dead_code)]
    pub fn random(nonce: u64) -> PubKey {
        let sks = tc::SecretKeySet::random(1, &mut rand::thread_rng());
        let set = sks.public_keys();
        let node = set.public_key_share(nonce);
        PubKey { set, node, nonce }
    }
    /// Temporary escape hatch to generate a `PubKey` from a `SecretKeySet` and a node id
    ///
    /// This _will_ be removed when shared secret generation is implemented. For now, it exists to
    /// solve the resulting chicken and egg problem.
    #[allow(clippy::similar_names)]
    pub fn from_secret_key_set_escape_hatch(sks: &tc::SecretKeySet, node_id: u64) -> Self {
        let pks = sks.public_keys();
        let tc_pub_key = pks.public_key_share(node_id);
        PubKey {
            set: pks,
            node: tc_pub_key,
            nonce: node_id,
        }
    }
}

impl PartialOrd for PubKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.nonce.partial_cmp(&other.nonce)
    }
}

impl Ord for PubKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.nonce.cmp(&other.nonce)
    }
}

/// Private key stub type
///
/// Opaque wrapper around `threshold_crypto` key
#[derive(Clone, Debug)]
pub struct PrivKey {
    /// This node's share of the overall secret key
    pub node: tc::SecretKeyShare,
}

impl PrivKey {
    /// Uses this private key to produce a partial signature for the given block hash
    #[must_use]
    pub fn partial_sign<const N: usize>(
        &self,
        hash: &LeafHash<N>,
        stage: Stage,
        view: ViewNumber,
    ) -> tc::SignatureShare {
        let blockhash = create_verify_hash(hash, view, stage);
        self.node.sign(blockhash)
    }
}
