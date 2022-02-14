//! Provides types useful for representing [`PhaseLock`](crate::PhaseLock)'s data structures
//!
//! This module provides types for representing consensus internal state, such as the [`Leaf`],
//! [`PhaseLock`](crate::PhaseLock)'s version of a block, and the [`QuorumCertificate`],
//! representing the threshold signatures fundamental to consensus.

use blake3::Hasher;
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
    pub parent: LeafHash<N>,
    /// The block contained in this `Leaf`
    pub item: T,
}

impl<T: BlockContents<N>, const N: usize> Leaf<T, N> {
    /// Creates a new leaf with the specified block and parent
    ///
    /// # Arguments
    ///   * `item` - The block to include
    ///   * `parent` - The hash of the `Leaf` that is to be the parent of this `Leaf`
    pub fn new(item: T, parent: LeafHash<N>) -> Self {
        Leaf { parent, item }
    }

    /// Hashes the leaf with the hashing algorithm provided by the [`BlockContents`] implementation
    ///
    /// This will concatenate the `parent` hash with the [`BlockContents`] provided hash of the
    /// contained block, and then return the hash of the resulting concatenated byte string.
    pub fn hash(&self) -> LeafHash<N> {
        let mut bytes = Vec::<u8>::new();
        bytes.extend_from_slice(self.parent.as_ref());
        bytes.extend_from_slice(BlockContents::hash(&self.item).as_ref());
        T::hash_leaf(&bytes)
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
    /// Hash of the [`Leaf`] referred to by this Quorum Certificate
    ///
    /// This value is covered by the threshold signature.
    pub(crate) leaf_hash: LeafHash<N>,
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
    #[must_use]
    pub fn verify(&self, key: &tc::PublicKeySet, view: u64, stage: Stage) -> bool {
        if let Some(signature) = &self.signature {
            let concatenated_hash = create_verify_hash(&self.leaf_hash, view, stage);
            key.public_key().verify(signature, &concatenated_hash)
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

/// This concatenates the encoding of `leaf_hash`, `view`, and `stage`, in
/// that order, and hashes the result.
pub fn create_verify_hash<const N: usize>(
    leaf_hash: &LeafHash<N>,
    view: u64,
    stage: Stage,
) -> VerifyHash<32> {
    let mut hasher = Hasher::new();
    hasher.update(leaf_hash.as_ref());
    hasher.update(&view.to_be_bytes());
    hasher.update(&(stage as u64).to_be_bytes());
    let hash = hasher.finalize();
    VerifyHash::from_array(*hash.as_bytes())
}

/// generates boilerplate code for any wrapper types
/// around `InternalHash`
macro_rules! gen_hash_wrapper_type {
    ($t:ident) => {
        #[derive(PartialEq, Eq, Clone, Copy, Hash, Serialize, Deserialize)]
        ///  External wrapper type
        pub struct $t<const N: usize> {
            inner: InternalHash<N>,
        }

        impl<const N: usize> $t<N> {
            /// Converts an array of the correct size directly into an `Self`
            pub fn from_array(input: [u8; N]) -> Self {
                $t {
                    inner: InternalHash::from_array(input),
                }
            }
            /// Clones the contents of this Hash into a `Vec<u8>`
            pub fn to_vec(self) -> Vec<u8> {
                self.inner.to_vec()
            }
            #[cfg(test)]
            pub fn random() -> Self {
                $t {
                    inner: InternalHash::random(),
                }
            }
        }
        impl<const N: usize> AsRef<[u8]> for $t<N> {
            fn as_ref(&self) -> &[u8] {
                self.inner.as_ref()
            }
        }

        impl<const N: usize> From<[u8; N]> for $t<N> {
            fn from(input: [u8; N]) -> Self {
                Self::from_array(input)
            }
        }

        impl<const N: usize> Default for $t<N> {
            fn default() -> Self {
                $t {
                    inner: InternalHash::default(),
                }
            }
        }
        impl<const N: usize> Debug for $t<N> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(&format!("{}", std::any::type_name::<$t<N>>()))
                    .field("inner", &format!("{}", HexFmt(&self.inner)))
                    .finish()
            }
        }
    };
}

gen_hash_wrapper_type!(BlockHash);
gen_hash_wrapper_type!(LeafHash);
gen_hash_wrapper_type!(TransactionHash);
gen_hash_wrapper_type!(VerifyHash);
gen_hash_wrapper_type!(StateHash);

/// Internal type used for representing hashes
///
/// This is a thin wrapper around a `[u8; N]` used to work around various issues with libraries that
/// have not updated to be const-generic aware. In particular, this provides a `serde` [`Serialize`]
/// and [`Deserialize`] implementation over the const-generic array, which `serde` normally does not
/// have for the general case.
#[derive(PartialEq, Eq, Clone, Copy, Hash, Serialize, Deserialize)]
struct InternalHash<const N: usize> {
    /// The underlying array
    /// No support for const generics
    #[serde(with = "serde_bytes_array")]
    inner: [u8; N],
}

impl<const N: usize> Debug for InternalHash<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternalHash")
            .field("inner", &format!("{}", HexFmt(&self.inner)))
            .finish()
    }
}

impl<const N: usize> InternalHash<N> {
    /// Converts an array of the correct size directly into an `InternalHash`
    pub const fn from_array(input: [u8; N]) -> Self {
        Self { inner: input }
    }

    /// Clones the contents of this `InternalHash` into a `Vec<u8>`
    pub fn to_vec(self) -> Vec<u8> {
        self.inner.to_vec()
    }

    /// Testing only random generation of a `InternalHash`
    #[cfg(test)]
    pub fn random() -> Self {
        use rand::Rng;
        let mut array = [0_u8; N];
        let mut rng = rand::thread_rng();
        rng.fill(&mut array[..]);
        Self { inner: array }
    }
}

impl<const N: usize> AsRef<[u8]> for InternalHash<N> {
    fn as_ref(&self) -> &[u8] {
        &self.inner
    }
}

impl<const N: usize> From<[u8; N]> for InternalHash<N> {
    fn from(input: [u8; N]) -> Self {
        Self::from_array(input)
    }
}

impl<const N: usize> Default for InternalHash<N> {
    fn default() -> Self {
        InternalHash {
            inner: [0_u8; { N }],
        }
    }
}

/// [Needed](https://github.com/serde-rs/bytes/issues/26#issuecomment-902550669) to (de)serialize const generic arrays
mod serde_bytes_array {
    use core::convert::TryInto;

    use serde::de::Error;
    use serde::{Deserializer, Serializer};

    /// This just specializes [`serde_bytes::serialize`] to `<T = [u8]>`.
    pub(crate) fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serde_bytes::serialize(bytes, serializer)
    }

    /// This takes the result of [`serde_bytes::deserialize`] from `[u8]` to `[u8; N]`.
    pub(crate) fn deserialize<'de, D, const N: usize>(deserializer: D) -> Result<[u8; N], D::Error>
    where
        D: Deserializer<'de>,
    {
        let slice: &[u8] = serde_bytes::deserialize(deserializer)?;
        let array: [u8; N] = slice.try_into().map_err(|_| {
            let expected = format!("[u8; {}]", N);
            D::Error::invalid_length(slice.len(), &expected.as_str())
        })?;
        Ok(array)
    }
}
