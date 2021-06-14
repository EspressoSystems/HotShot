use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};

use std::fmt::Debug;
use threshold_crypto as tc;

use crate::{BlockContents, BlockHash};

#[derive(Serialize, Deserialize, Clone)]
/// A node in `HotStuff`'s tree
pub struct Leaf<T> {
    /// The hash of the parent
    pub parent: BlockHash,
    /// The item in the node
    pub item: T,
}

impl<T: BlockContents> Leaf<T> {
    /// Creates a new leaf with the specified contents
    pub fn new(item: T, parent: BlockHash) -> Self {
        Leaf { parent, item }
    }

    /// Hashes the leaf with Blake3
    pub fn hash(&self) -> BlockHash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.parent);
        hasher.update(&BlockContents::hash(&self.item));
        *hasher.finalize().as_bytes()
    }
}

impl<T: Debug> Debug for Leaf<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(&format!("Leaf<{}>", std::any::type_name::<T>()))
            .field("item", &self.item)
            .field("parent", &format!("{:12}", HexFmt(&self.parent)))
            .finish()
    }
}

/// The type used for quorum certs
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct QuorumCertificate {
    /// Block this QC refers to
    pub(crate) hash: BlockHash,
    /// The view we were on when we made this certificate
    pub(crate) view_number: u64,
    /// The stage of consensus we were on when we made this certificate
    pub(crate) stage: Stage,
    /// The signature portion of this QC
    pub(crate) signature: Option<tc::Signature>,
    /// Temporary bypass for boostrapping
    pub(crate) genesis: bool,
}

impl QuorumCertificate {
    /// Verifies a quorum certificate
    #[must_use]
    pub fn verify(&self, key: &tc::PublicKeySet, stage: Stage, view: u64) -> bool {
        // Temporary, stage and view should be included in signature in future
        if let Some(signature) = &self.signature {
            key.public_key().verify(&signature, &self.hash)
                && self.stage == stage
                && self.view_number == view
        } else {
            self.genesis
        }
    }
}

impl Debug for QuorumCertificate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuorumCertificate")
            .field("hash", &format!("{:12}", HexFmt(&self.hash)))
            .field("view_number", &self.view_number)
            .field("stage", &self.stage)
            .field("signature", &self.signature)
            .field("genesis", &self.genesis)
            .finish()
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
/// Represents the stages of consensus
pub enum Stage {
    /// Between rounds
    None,
    /// Prepare Phase
    Prepare,
    /// PreCommit Phase
    PreCommit,
    /// Commit Phase
    Commit,
    /// Decide Phase
    Decide,
}
