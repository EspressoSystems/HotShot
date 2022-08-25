//! Provides types useful for representing `HotShot`'s data structures
//!
//! This module provides types for representing consensus internal state, such as the [`Leaf`],
//! `HotShot`'s version of a block, and the [`QuorumCertificate`], representing the threshold
//! signatures fundamental to consensus.
use crate::traits::{
    signature_key::{EncodedPublicKey, EncodedSignature},
    BlockContents, StateContents,
};
use arbitrary::Arbitrary;
use blake3::Hasher;
use commit::{Commitment, Committable};
use hex_fmt::HexFmt;
use hotshot_utils::hack::nll_todo;
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

impl<STATE: StateContents> Default for QuorumCertificate<STATE> {
    fn default() -> Self {
        Self {
            block_commitment: nll_todo(),
            leaf_commitment: nll_todo(),
            view_number: nll_todo(),
            signatures: nll_todo(),
            genesis: nll_todo(),
        }
    }
}

/// The type used for Quorum Certificates
///
/// A Quorum Certificate is a threshold signature of the [`Leaf`] being proposed, as well as some
/// metadata, such as the [`Stage`] of consensus the quorum certificate was generated during.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, custom_debug::Debug, std::hash::Hash)]
pub struct QuorumCertificate<STATE: StateContents> {
    /// Hash of the block refereed to by this Quorum Certificate.
    ///
    /// This is included for convenience, and is not fundamental to consensus or covered by the
    /// signature. This _must_ be identical to the [`BlockContents`] provided hash of the `item` in
    /// the referenced leaf.
    #[debug(skip)]
    #[serde(deserialize_with = "<Commitment<STATE::Block> as Deserialize>::deserialize")]
    pub block_commitment: Commitment<STATE::Block>,

    /// Hash of the [`Leaf`] referred to by this Quorum Certificate
    ///
    /// This value is covered by the threshold signature.
    #[debug(skip)]
    #[serde(deserialize_with = "<Commitment<Leaf<STATE>> as Deserialize>::deserialize")]
    pub leaf_commitment: Commitment<Leaf<STATE>>,

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

impl<STATE: StateContents> Committable for QuorumCertificate<STATE> {
    fn commit(&self) -> Commitment<Self> {
        commit::RawCommitmentBuilder::new("QC Comm")
            .constant_str("view_number")
            .u64(*self.view_number)
            .field("block commitment", self.block_commitment)
            .field("leaf commitment", self.leaf_commitment)
            .constant_str("signatures")
            // TODO not sure what to for this
            // do we need to hash this?. I think other fields should be enough.
            .var_size_bytes(nll_todo())
            .constant_str("genesis")
            .u64(self.genesis as u64)
            .finalize()
    }
}

/// A node in `HotShot`'s consensus-internal merkle tree.
///
/// This is the consensus-internal analogous concept to a block, and it contains the block proper,
/// as well as the hash of its parent `Leaf`.
/// NOTE: `T` is constrainted to implementing `BlockContents`, is `TypeMap::Block`
#[derive(Clone, Serialize, Deserialize, custom_debug::Debug, PartialEq, std::hash::Hash, Eq)]
pub struct Leaf<STATE: StateContents> {
    /// CurView from leader when proposing leaf
    pub view_number: ViewNumber,

    /// Per spec, justification
    #[serde(deserialize_with = "<QuorumCertificate<STATE> as Deserialize>::deserialize")]
    pub justify_qc: QuorumCertificate<STATE>,

    /// The hash of the parent `Leaf`
    /// So we can ask if it extends
    #[debug(skip)]
    #[serde(deserialize_with = "<Commitment<Leaf<STATE>> as Deserialize>::deserialize")]
    pub parent: Commitment<Leaf<STATE>>,

    /// Block leaf wants to apply
    #[serde(deserialize_with = "STATE::Block::deserialize")]
    pub deltas: STATE::Block,

    /// What the state should be after applying `self.deltas`
    #[serde(deserialize_with = "STATE::deserialize")]
    pub state: STATE,
}

impl<STATE: StateContents> Committable for Leaf<STATE> {
    fn commit(&self) -> commit::Commitment<Self> {
        commit::RawCommitmentBuilder::new("Leaf Comm")
            .constant_str("view_number")
            .u64(*self.view_number)
            .field("justify_qc", self.justify_qc.commit())
            .field("parent Leaf commitment", self.parent)
            .field("deltas commitment", self.deltas.commit())
            .field("state commitment", self.state.commit())
            .finalize()
    }
}

impl<STATE: StateContents> Leaf<STATE> {
    /// Creates a new leaf with the specified block and parent
    ///
    /// # Arguments
    ///   * `item` - The block to include
    ///   * `parent` - The hash of the `Leaf` that is to be the parent of this `Leaf`
    pub fn new(
        state: STATE,
        deltas: STATE::Block,
        parent: Commitment<Leaf<STATE>>,
        qc: QuorumCertificate<STATE>,
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
}

/// Format a fixed-size array with [`HexFmt`]
fn fmt_arr<const N: usize>(n: &[u8; N], f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", HexFmt(n))
}
