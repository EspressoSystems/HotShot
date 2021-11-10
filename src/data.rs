//! Provides types useful for representing [`PhaseLock`](crate::PhaseLock)'s data structures
//!
//! This module provides types for representing consensus internal state, such as the [`Leaf`],
//! [`PhaseLock`](crate::PhaseLock)'s version of a block, and the [`QuorumCertificate`],
//! representing the threshold signatures fundamental to consensus.

use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};

use std::fmt::Debug;
use threshold_crypto as tc;

use crate::traits::BlockContents;

#[derive(Serialize, Deserialize, Clone)]
/// A node in [`PhaseLock`](crate::PhaseLock)'s consensus-internal merkle tree.
///
/// This is the consensus-internal analogous concept to a block, and it contains the block proper,
/// as well as the hash of its parent `Leaf`.
pub struct Leaf<T, const N: usize> {
    /// The hash of the parent `Leaf`
    pub parent: BlockHash<N>,
    /// The block contained in this `Leaf`
    pub item: T,
}

impl<T: BlockContents<N>, const N: usize> Leaf<T, N> {
    /// Creates a new leaf with the specified block and parent
    ///
    /// # Arguments
    ///   * `item` - The block to include
    ///   * `parent` - The hash of the `Leaf` that is to be the parent of this `Leaf`
    pub fn new(item: T, parent: BlockHash<N>) -> Self {
        Leaf { parent, item }
    }

    /// Hashes the leaf with the hashing algorithm provided by the [`BlockContents`] implementation
    ///
    /// This will concatenate the `parent` hash with the [`BlockContents`] provided hash of the
    /// contained block, and then return the hash of the resulting concatenated byte string.
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

/// The type used for Quorum Certificates
///
/// A Quorum Certificate is a threshold signature of the [`Leaf`] being proposed, as well as some
/// metadata, such as the [`Stage`] of consensus the quorum certificate was generated during.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct QuorumCertificate<const N: usize> {
    /// Hash of the block refereed to by this Quorum Certificate.
    ///
    /// This is included for convenience, and is not fundamental to consensus or covered by the
    /// signature. This _must_ be identical to the [`BlockContents`] provided hash of the `item` in
    /// the referenced leaf.
    pub(crate) block_hash: BlockHash<N>,
    /// Hash of the [`Leaf`] refereed to by this Quorum Certificate
    ///
    /// This value is covered by the threshold signature.
    pub(crate) leaf_hash: BlockHash<N>,
    /// The view number this quorum certificate was generated during
    ///
    /// This value is covered by the threshold signature.
    pub(crate) view_number: u64,
    /// The [`Stage`] of consensus that this Quorum Certificate was generated during
    ///
    /// This value is covered by the threshold signature.
    pub(crate) stage: Stage,
    /// The threshold signature associated with this Quorum Certificate.
    ///
    /// This is nullable as part of a temporary mechanism to support bootstrapping from a genesis block, as
    /// the genesis block can not be produced through the normal means.
    pub(crate) signature: Option<tc::Signature>,
    /// Temporary bypass for boostrapping
    ///
    /// This value indicates that this is a dummy certificate for the genesis block, and thus does not have
    /// a signature. This value is not covered by the signature, and it is invalid for this to be set
    /// outside of bootstrap
    pub(crate) genesis: bool,
}

impl<const N: usize> QuorumCertificate<N> {
    /// Verifies a quorum certificate
    ///
    /// This concatenates the encoding of the [`Leaf`] hash, the `view_number`, and the `stage`, in
    /// that order, and makes sure that the associated signature validates against the resulting
    /// byte string.
    ///
    /// If the `genesis` value is set, this disables the normal checking, and instead performs
    /// bootstrap checking.
    ///
    /// TODO([#22](https://gitlab.com/translucence/systems/phaselock/-/issues/22)): This needs to
    /// include the stage and view in the signature
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

    /// Converts this Quorum Certificate to a version using a `Vec` rather than a const-generic
    /// array.
    ///
    /// This is useful for erasing the const-generic length for error types and logging, but is not
    /// directly consensus relevant.
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

/// [`QuorumCertificate`] variant using a `Vec` rather than a const-generic array
///
/// This type mainly exists to work around an issue with
/// [`snafu`](https://github.com/shepmaster/snafu) when used with const-generics, by erasing the
/// const-generic length.
///
/// This type is not used directly by consensus.
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

/// Type used for representing hashes
///
/// This is a thin wrapper around a `[u8; N]` used to work around various issues with libraries that
/// have not updated to be const-generic aware. In particular, this provides a `serde` [`Serialize`]
/// and [`Deserialize`] implementation over the const-generic array, which `serde` normally does not
/// have for the general case.
///
/// TODO([#36](https://gitlab.com/translucence/systems/phaselock/-/issues/36)) Break this up into a
/// core type and type-stating wrappers, and utilize `serde_bytes` instead of the visitor
/// implementation
#[derive(PartialEq, Eq, Debug, Clone, Copy, Hash)]
pub struct BlockHash<const N: usize> {
    /// The underlying array
    inner: [u8; N],
}

impl<const N: usize> BlockHash<N> {
    /// Converts an array of the correct size directly into a `BlockHash`
    pub const fn from_array(input: [u8; N]) -> Self {
        Self { inner: input }
    }

    /// Clones the contents of this `BlockHash` into a `Vec<u8>`
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
