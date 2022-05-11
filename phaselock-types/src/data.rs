//! Provides types useful for representing `PhaseLock`'s data structures
//!
//! This module provides types for representing consensus internal state, such as the [`Leaf`],
//! `PhaseLock`'s version of a block, and the [`QuorumCertificate`], representing the threshold
//! signatures fundamental to consensus.
use crate::traits::{signature_key::SignatureKey, BlockContents};
use blake3::Hasher;
use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::{collections::HashSet, fmt::Debug};

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

    use serde::de::Error;
    use serde::{Deserializer, Serializer};

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

/// The type used for Quorum Certificates
///
/// A Quorum Certificate is a threshold signature of the [`Leaf`] being proposed, as well as some
/// metadata, such as the [`Stage`] of consensus the quorum certificate was generated during.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, custom_debug::Debug)]
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
    /// The [`Stage`] of consensus that this Quorum Certificate was generated during
    ///
    /// This value is covered by the threshold signature.
    pub stage: Stage,
    /// The list of signatures establishing the vailidity of this Quorum
    /// Certificate.
    ///
    /// These are binary encoded signatures made by the `NodeImplementation`
    /// specified singature type
    pub signatures: Vec<Vec<u8>>,
    /// Temporary bypass for boostrapping
    ///
    /// This value indicates that this is a dummy certificate for the genesis block, and thus does
    /// not have a signature. This value is not covered by the signature, and it is invalid for this
    /// to be set outside of bootstrap
    pub genesis: bool,
}

impl<const N: usize> QuorumCertificate<N> {
    /// Verifies a quorum certificate
    ///
    /// This concatenates the encoding of the [`Leaf`] hash, the `view_number`,
    /// and the `stage`, in that order, and makes sure that at least the
    /// threshold number of the included signatures both:
    ///  - Are made by keys in the allowed key set
    ///  - Validate against the hash
    ///
    /// If the `genesis` value is set, this disables the normal checking, and
    /// instead performs bootstrap checking.
    ///
    /// FIXME(#170): This needs to be hooked into the election trait to allow
    /// it to verify the signatures, rather than relying on a set list, as the
    /// list of particpiants in a given view number is, strictly speaking,
    /// unknowable with committee election
    #[must_use]
    #[allow(clippy::if_not_else)] // This one is more readable this way
    pub fn verify<S: SignatureKey>(
        &self,
        keys: &HashSet<S>,
        threshold: u64,
        view: ViewNumber,
        stage: Stage,
    ) -> bool {
        if !self.signatures.is_empty() {
            let concatenated_hash = create_verify_hash(&self.leaf_hash, view, stage);
            // FIXME(#170): This is a really in efficent method, we should
            // include the public key in the signature
            let valid_signatures = self
                .signatures
                .iter()
                .filter(|signature| {
                    for key in keys {
                        if key.validate(signature, concatenated_hash.as_ref()) {
                            return true;
                        }
                    }
                    false
                })
                .count();
            valid_signatures as u64 >= threshold
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
    pub hash: Vec<u8>,
    /// The view we were on when we made this certificate
    pub view_number: ViewNumber,
    /// The stage of consensus we were on when we made this certificate
    pub stage: Stage,
    /// The signature portion of this QC
    pub signatures: Vec<Vec<u8>>,
    /// Temporary bypass for boostrapping
    pub genesis: bool,
}

/// This concatenates the encoding of `leaf_hash`, `view`, and `stage`, in
/// that order, and hashes the result.
pub fn create_verify_hash<const N: usize>(
    leaf_hash: &LeafHash<N>,
    view: ViewNumber,
    stage: Stage,
) -> VerifyHash<32> {
    let mut hasher = Hasher::new();
    hasher.update(leaf_hash.as_ref());
    hasher.update(&view.to_be_bytes());
    hasher.update(&(stage as u64).to_be_bytes());
    let hash = hasher.finalize();
    VerifyHash::from_array(*hash.as_bytes())
}

/// A node in `PhaseLock`'s consensus-internal merkle tree.
///
/// This is the consensus-internal analogous concept to a block, and it contains the block proper,
/// as well as the hash of its parent `Leaf`.
#[derive(Serialize, Deserialize, Clone, custom_debug::Debug, PartialEq)]
pub struct Leaf<T, const N: usize> {
    /// The hash of the parent `Leaf`
    #[debug(with = "fmt_leaf_hash")]
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

#[test]
fn test_validate_qc() {
    // Validate that `qc.verify` and `ConsensusMessage::validate_qc` both validate correctly.
    use crate::{
        message::{Commit, ConsensusMessage, Decide, NewView, PreCommit, Prepare, Vote},
        traits::{block_contents::dummy::DummyBlock, state::dummy::DummyState},
        PrivKey, PubKey,
    };
    use tc::SecretKeySet;

    let block_hash = BlockHash::<32>::random();
    let leaf_hash = LeafHash::<32>::random();

    let sks = SecretKeySet::random(4, &mut rand::thread_rng());
    let public_key = PubKey::from_secret_key_set_escape_hatch(&sks, 0);
    let view = ViewNumber::new(5);

    for stage in [
        Stage::Prepare,
        Stage::PreCommit,
        Stage::Commit,
        Stage::Decide,
    ] {
        let votes = (1..=5)
            .map(|id| {
                let privkey = PrivKey {
                    node: sks.secret_key_share(id),
                };
                Vote {
                    current_view: view,
                    id,
                    leaf_hash,
                    signature: privkey.partial_sign(&leaf_hash, stage, view),
                }
            })
            .collect::<Vec<_>>();

        let signature = public_key
            .set
            .combine_signatures(votes.iter().map(|v| (v.id, &v.signature)))
            .unwrap();
        let qc = QuorumCertificate {
            block_hash,
            leaf_hash,
            view_number: view,
            stage,
            signature: Some(signature),
            genesis: false,
        };

        assert!(qc.verify(&public_key.set, view, stage));

        match stage {
            Stage::Prepare => {
                assert!(
                    ConsensusMessage::<DummyBlock, DummyState, 32>::NewView(NewView {
                        current_view: view,
                        justify: qc.clone()
                    })
                    .validate_qc(&public_key.set)
                );
                assert!(ConsensusMessage::Prepare(Prepare {
                    current_view: view,
                    leaf: Leaf::new(DummyBlock::random(), leaf_hash),
                    state: DummyState::random(),
                    high_qc: qc
                })
                .validate_qc(&public_key.set));
            }
            Stage::PreCommit => {
                assert!(
                    ConsensusMessage::<DummyBlock, DummyState, 32>::PreCommit(PreCommit {
                        current_view: view,
                        leaf_hash,
                        qc
                    })
                    .validate_qc(&public_key.set)
                );
            }
            Stage::Commit => {
                assert!(
                    ConsensusMessage::<DummyBlock, DummyState, 32>::Commit(Commit {
                        current_view: view,
                        leaf_hash,
                        qc
                    })
                    .validate_qc(&public_key.set)
                );
            }
            Stage::Decide => {
                assert!(
                    ConsensusMessage::<DummyBlock, DummyState, 32>::Decide(Decide {
                        current_view: view,
                        leaf_hash,
                        qc
                    })
                    .validate_qc(&public_key.set)
                );
            }
            Stage::None => unreachable!(),
        }
    }
}
