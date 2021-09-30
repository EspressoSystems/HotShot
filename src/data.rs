use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};

use std::fmt::Debug;
use threshold_crypto as tc;

use crate::BlockContents;

#[derive(Serialize, Deserialize, Clone)]
/// A node in `PhaseLock`'s tree
pub struct Leaf<T, const N: usize> {
    /// The hash of the parent
    pub parent: BlockHash<N>,
    /// The item in the node
    pub item: T,
}

impl<T: BlockContents<N>, const N: usize> Leaf<T, N> {
    /// Creates a new leaf with the specified contents
    pub fn new(item: T, parent: BlockHash<N>) -> Self {
        Leaf { parent, item }
    }

    /// Hashes the leaf with Blake3
    ///
    /// TODO: Add hasher implementation to block contents trait
    pub fn hash(&self) -> BlockHash<N> {
        let mut bytes = Vec::<u8>::new();
        bytes.extend_from_slice(self.parent.as_ref());
        bytes.extend_from_slice(BlockContents::hash(&self.item).as_ref());
        T::hash_bytes(&bytes)
    }
}

impl<T: Debug, const N: usize> Debug for Leaf<T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(&format!("Leaf<{}>", std::any::type_name::<T>()))
            .field("item", &self.item)
            .field("parent", &format!("{:12}", HexFmt(&self.parent)))
            .finish()
    }
}

/// The type used for quorum certs
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct QuorumCertificate<const N: usize> {
    /// Block this QC refers to
    pub(crate) block_hash: BlockHash<N>,
    /// Leaf this QC refers to
    pub(crate) leaf_hash: BlockHash<N>,
    /// The view we were on when we made this certificate
    pub(crate) view_number: u64,
    /// The stage of consensus we were on when we made this certificate
    pub(crate) stage: Stage,
    /// The signature portion of this QC
    pub(crate) signature: Option<tc::Signature>,
    /// Temporary bypass for boostrapping
    pub(crate) genesis: bool,
}

impl<const N: usize> QuorumCertificate<N> {
    /// Verifies a quorum certificate
    #[must_use]
    pub fn verify(&self, key: &tc::PublicKeySet, stage: Stage, view: u64) -> bool {
        // Temporary, stage and view should be included in signature in future
        if let Some(signature) = &self.signature {
            key.public_key().verify(signature, &self.leaf_hash)
                && self.stage == stage
                && self.view_number == view
        } else {
            self.genesis
        }
    }

    /// Converts to a vector based cert
    pub fn to_vec_cert(&self) -> VecQuorumCertificate {
        VecQuorumCertificate {
            hash: self.block_hash.as_ref().to_vec(),
            view_number: self.view_number,
            stage: self.stage,
            signature: self.signature.clone(),
            genesis: self.genesis,
        }
    }
}

impl<const N: usize> Debug for QuorumCertificate<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuorumCertificate")
            .field("hash", &format!("{:12}", HexFmt(&self.leaf_hash)))
            .field("view_number", &self.view_number)
            .field("stage", &self.stage)
            .field("signature", &self.signature)
            .field("genesis", &self.genesis)
            .finish()
    }
}

/// Vectorized quorum cert, used for debugging
///
/// Mainly exists to work around issuse with snafu
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct VecQuorumCertificate {
    /// Block this QC refers to
    pub(crate) hash: Vec<u8>,
    /// The view we were on when we made this certificate
    pub(crate) view_number: u64,
    /// The stage of consensus we were on when we made this certificate
    pub(crate) stage: Stage,
    /// The signature portion of this QC
    pub(crate) signature: Option<tc::Signature>,
    /// Temporary bypass for boostrapping
    pub(crate) genesis: bool,
}

impl Debug for VecQuorumCertificate {
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

/// The type used for block hashes
///
/// Thin wrapper around a [u8; N] to provide serialize and deserialize functionality
#[derive(PartialEq, Eq, Debug, Clone, Copy, Hash)]
pub struct BlockHash<const N: usize> {
    /// The underlying array
    inner: [u8; N],
}

impl<const N: usize> BlockHash<N> {
    /// Converts an array of the correct size into a `BlockHash`
    pub const fn from_array(input: [u8; N]) -> Self {
        Self { inner: input }
    }

    /// Converts this `BlockHash` to a vector
    pub fn to_vec(&self) -> Vec<u8> {
        self.inner.to_vec()
    }

    /// Testing only random generation of a `BlockHash`
    #[cfg(test)]
    pub fn random() -> Self {
        use rand::Rng;
        let mut array = [0_u8; N];
        let mut rng = rand::thread_rng();
        rng.fill(&mut array[..]);
        Self { inner: array }
    }
}

impl<const N: usize> AsRef<[u8]> for BlockHash<N> {
    fn as_ref(&self) -> &[u8] {
        &self.inner
    }
}

impl<const N: usize> From<[u8; N]> for BlockHash<N> {
    fn from(input: [u8; N]) -> Self {
        Self::from_array(input)
    }
}

impl<const N: usize> Default for BlockHash<N> {
    fn default() -> Self {
        BlockHash {
            inner: [0_u8; { N }],
        }
    }
}

impl<const N: usize> Serialize for BlockHash<N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(&self.inner)
    }
}

impl<'de, const N: usize> Deserialize<'de> for BlockHash<N> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_bytes(BlockHashVisitor::<N>)
    }
}

/// `Visitor` implementation for deserializing `BlockHash`
struct BlockHashVisitor<const N: usize>;

impl<'de, const N: usize> serde::de::Visitor<'de> for BlockHashVisitor<N> {
    type Value = BlockHash<N>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "a byte array of length {}", { N })
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if v.len() == { N } {
            let mut result = [0_u8; { N }];
            result.copy_from_slice(v);
            Ok(BlockHash { inner: result })
        } else {
            let x = format!("{}", { N });
            Err(E::invalid_length(v.len(), &x.as_str()))
        }
    }
}
