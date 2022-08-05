//! Provides types useful for representing `HotShot`'s data structures
//!
//! This module provides types for representing consensus internal state, such as the [`Leaf`],
//! `HotShot`'s version of a block, and the [`QuorumCertificate`], representing the threshold
//! signatures fundamental to consensus.
use crate::traits::{
    signature_key::{EncodedPublicKey, EncodedSignature},
    BlockContents, State,
};
use blake3::Hasher;
use hex_fmt::HexFmt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Debug};

/// Type-safe wrapper around `u64` so we know the thing we're talking about is a view number.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ViewNumber(u64);

impl ViewNumber {
    /// Create a genesis view number (0)
    pub const fn genesis() -> Self {
        Self(0)
    }

    /// Create a new `ViewNumber` with the given value.
    pub const fn new(n: u64) -> Self {
        Self(n)
    }
}

impl std::ops::Add<u64> for ViewNumber {
    type Output = ViewNumber;

    fn add(self, rhs: u64) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl std::ops::AddAssign<u64> for ViewNumber {
    fn add_assign(&mut self, rhs: u64) {
        self.0 += rhs;
    }
}

impl std::ops::Deref for ViewNumber {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// generates boilerplate code for any wrapper types
/// around `InternalHash`
macro_rules! gen_hash_wrapper_type {
    ($t:ident) => {
        #[derive(PartialEq, Eq, Clone, Copy, Hash, Serialize, Deserialize, Ord, PartialOrd)]
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
            /// Testing only random generation
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
#[derive(
    PartialEq, Eq, Clone, Copy, Hash, Serialize, Deserialize, custom_debug::Debug, PartialOrd, Ord,
)]
pub struct InternalHash<const N: usize> {
    /// The underlying array
    /// No support for const generics
    #[serde(with = "serde_bytes_array")]
    #[debug(with = "fmt_arr")]
    inner: [u8; N],
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

    use serde::{de::Error, Deserializer, Serializer};

    /// This just specializes [`serde_bytes::serialize`] to `<T = [u8]>`.
    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serde_bytes::serialize(bytes, serializer)
    }

    /// This takes the result of [`serde_bytes::deserialize`] from `[u8]` to `[u8; N]`.
    pub fn deserialize<'de, D, const N: usize>(deserializer: D) -> Result<[u8; N], D::Error>
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

/// The type used for Quorum Certificates
///
/// A Quorum Certificate is a threshold signature of the [`Leaf`] being proposed, as well as some
/// metadata, such as the [`Stage`] of consensus the quorum certificate was generated during.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, custom_debug::Debug, std::hash::Hash)]
pub struct QuorumCertificate<const N: usize> {
    /// Hash of the block refereed to by this Quorum Certificate.
    ///
    /// This is included for convenience, and is not fundamental to consensus or covered by the
    /// signature. This _must_ be identical to the [`BlockContents`] provided hash of the `item` in
    /// the referenced leaf.
    #[debug(with = "fmt_blockhash")]
    pub block_hash: BlockHash<N>,

    /// Hash of the [`Leaf`] referred to by this Quorum Certificate
    ///
    /// This value is covered by the threshold signature.
    #[debug(skip)]
    pub leaf_hash: LeafHash<N>,

    /// The view number this quorum certificate was generated during
    ///
    /// This value is covered by the threshold signature.
    pub view_number: ViewNumber,

    /// The list of signatures establishing the validity of this Quorum Certifcate
    ///
    /// This is a mapping of the byte encoded public keys provided by the [`NodeImplementation`], to
    /// the byte encoded signatures provided by those keys.
    ///
    /// These formats are deliberatly done as a `Vec` instead of an array to prevent creating the
    /// assumption that singatures are constant in length
    pub signatures: BTreeMap<EncodedPublicKey, EncodedSignature>,

    /// Temporary bypass for boostrapping
    ///
    /// This value indicates that this is a dummy certificate for the genesis block, and thus does
    /// not have a signature. This value is not covered by the signature, and it is invalid for this
    /// to be set outside of bootstrap
    pub genesis: bool,
}

impl<const N: usize> QuorumCertificate<N> {
    /// Converts this Quorum Certificate to a version using a `Vec` rather than a const-generic
    /// array.
    ///
    /// This is useful for erasing the const-generic length for error types and logging, but is not
    /// directly consensus relevant.
    pub fn to_vec_cert(&self) -> VecQuorumCertificate {
        VecQuorumCertificate {
            block_hash: self.block_hash.as_ref().to_vec(),
            view_number: self.view_number,
            signatures: self.signatures.clone(),
            genesis: self.genesis,
        }
    }
}

/// [`QuorumCertificate`] variant using a `Vec` rather than a const-generic array
///
/// This type mainly exists to work around an issue with
/// [`snafu`](https://github.com/shepmaster/snafu) when used with const-generics, by erasing the
/// const-generic length.
///
/// This type is not used directly by consensus.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, custom_debug::Debug)]
pub struct VecQuorumCertificate {
    /// Block this QC refers to
    #[debug(with = "fmt_vec")]
    pub block_hash: Vec<u8>,
    /// The view we were on when we made this certificate
    pub view_number: ViewNumber,
    /// The signature portion of this QC
    pub signatures: BTreeMap<EncodedPublicKey, EncodedSignature>,
    /// Temporary bypass for boostrapping
    pub genesis: bool,
}

impl VecQuorumCertificate {
    /// Create a dummy [`VecQuorumCertificate`]
    pub fn dummy<const N: usize>() -> Self {
        Self {
            block_hash: BlockHash::<N>::random().to_vec(),
            view_number: ViewNumber::genesis(),
            signatures: BTreeMap::default(),
            genesis: false,
        }
    }
}

/// This concatenates the encoding of `leaf_hash`, `view`, and `stage`, in
/// that order, and hashes the result.
pub fn create_verify_hash<const N: usize>(
    leaf_hash: &LeafHash<N>,
    view: ViewNumber,
) -> VerifyHash<32> {
    let mut hasher = Hasher::new();
    hasher.update(leaf_hash.as_ref());
    hasher.update(&view.to_be_bytes());
    let hash = hasher.finalize();
    VerifyHash::from_array(*hash.as_bytes())
}

/// A node in `HotShot`'s consensus-internal merkle tree.
///
/// This is the consensus-internal analogous concept to a block, and it contains the block proper,
/// as well as the hash of its parent `Leaf`.
/// NOTE: T is constrainted to implementing BlockContents, is TypeMap::Block
#[derive(Clone, Serialize, Deserialize, custom_debug::Debug, PartialEq, std::hash::Hash, Eq)]
pub struct Leaf<STATE, BLOCK, const N: usize> {
    /// Per spec
    pub view_number: ViewNumber,

    /// Per spec, justification
    pub justify_qc: QuorumCertificate<N>,

    /// The hash of the parent `Leaf`
    /// So we can ask if it extends
    #[debug(with = "fmt_leaf_hash")]
    pub parent: LeafHash<N>,

    /// Block leaf wants to apply
    pub deltas: BLOCK,

    // What the state should be after applying `self.deltas`
    pub state: STATE,
}

impl<STATE: BlockContents<N>, BLOCK: BlockContents<N>, const N: usize> Leaf<STATE, BLOCK, N> {
    /// Creates a new leaf with the specified block and parent
    ///
    /// # Arguments
    ///   * `item` - The block to include
    ///   * `parent` - The hash of the `Leaf` that is to be the parent of this `Leaf`
    pub fn new(
        state: STATE,
        deltas: BLOCK,
        parent: LeafHash<N>,
        qc: QuorumCertificate<N>,
        view_number: ViewNumber,
    ) -> Self {
        Leaf {
            view_number,
            justify_qc: qc,
            parent,
            deltas,
            state,
        }
    }

    /// Hashes the leaf with the hashing algorithm provided by the [`BlockContents`] implementation
    ///
    /// This will concatenate the `parent` hash with the [`BlockContents`] provided hash of the
    /// contained block, and then return the hash of the resulting concatenated byte string.
    /// NOTE: are we sure this is hashing correctly
    pub fn hash(&self) -> LeafHash<N> {
        let mut bytes = Vec::<u8>::new();
        bytes.extend_from_slice(self.parent.as_ref());
        bytes.extend_from_slice(<BLOCK as BlockContents<N>>::hash(&self.deltas).as_ref());
        <BLOCK as BlockContents<N>>::hash_leaf(&bytes)
    }
}

/// Format a fixed-size array with [`HexFmt`]
fn fmt_arr<const N: usize>(n: &[u8; N], f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", HexFmt(n))
}

/// Format a vec with [`HexFmt`]
#[allow(clippy::ptr_arg)] // required because `custom_debug` requires an exact type match
fn fmt_vec(n: &Vec<u8>, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{:12}", HexFmt(n))
}

/// Format a [`BlockHash`] with [`HexFmt`]
fn fmt_blockhash<const N: usize>(
    n: &BlockHash<N>,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    write!(f, "{:12}", HexFmt(n))
}

/// Format a [`LeafHash`] with [`HexFmt`]
fn fmt_leaf_hash<const N: usize>(
    n: &LeafHash<N>,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    write!(f, "{:12}", HexFmt(n))
}
