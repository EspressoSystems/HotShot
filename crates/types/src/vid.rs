//! This module provides:
//! - an opaque constructor [`vid_scheme`] that returns a new instance of a
//!   VID scheme.
//! - type aliases [`VidCommitment`], [`VidCommon`], [`VidShare`]
//!   for [`VidScheme`] assoc types.
//!
//! Purpose: the specific choice of VID scheme is an implementation detail.
//! This crate and all downstream crates should talk to the VID scheme only
//! via the traits exposed here.

use ark_bn254::Bn254;
use jf_primitives::{
    pcs::{
        checked_fft_size,
        prelude::{UnivariateKzgPCS, UnivariateUniversalParams},
        PolynomialCommitmentScheme,
    },
    vid::{
        advz::{
            self,
            payload_prover::{LargeRangeProof, SmallRangeProof},
        },
        payload_prover::{PayloadProver, Statement},
        precomputable::Precomputable,
        VidDisperse, VidResult, VidScheme,
    },
};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{fmt::Debug, ops::Range};

/// VID scheme constructor.
///
/// Returns an opaque type that impls jellyfish traits:
/// [`VidScheme`], [`PayloadProver`], [`Precomputable`].
///
/// # Rust forbids naming impl Trait in return types
///
/// Due to Rust limitations the return type of [`vid_scheme`] is a newtype
/// wrapper [`VidSchemeType`] that impls the above traits.
///
/// We prefer that the return type of [`vid_scheme`] be `impl Trait` for the
/// above traits. But the ability to name an impl Trait return type is
/// currently missing from Rust:
/// - [Naming impl trait in return types - Impl trait initiative](https://rust-lang.github.io/impl-trait-initiative/explainer/rpit_names.html)
/// - [RFC: Type alias impl trait (TAIT)](https://github.com/rust-lang/rfcs/blob/master/text/2515-type_alias_impl_trait.md)
///
/// # Panics
/// When the construction fails for the underlying VID scheme.
#[must_use]
pub fn vid_scheme(num_storage_nodes: usize) -> VidSchemeType {
    // chunk_size is currently num_storage_nodes rounded down to a power of two
    // TODO chunk_size should be a function of the desired erasure code rate
    // https://github.com/EspressoSystems/HotShot/issues/2152
    let chunk_size = 1 << num_storage_nodes.ilog2();

    // TODO intelligent choice of multiplicity
    let multiplicity = 1;

    // TODO panic, return `Result`, or make `new` infallible upstream (eg. by panicking)?
    #[allow(clippy::panic)]
    VidSchemeType(Advz::new(chunk_size, num_storage_nodes, multiplicity, &*KZG_SRS).unwrap_or_else(|err| panic!("advz construction failure:\n\t(num_storage nodes,chunk_size,multiplicity)=({num_storage_nodes},{chunk_size},{multiplicity})\n\terror: : {err}")))
}

/// VID commitment type
pub type VidCommitment = <VidSchemeType as VidScheme>::Commit;
/// VID common type
pub type VidCommon = <VidSchemeType as VidScheme>::Common;
/// VID share type
pub type VidShare = <VidSchemeType as VidScheme>::Share;

#[cfg(not(feature = "gpu-vid"))]
/// Internal Jellyfish VID scheme
type Advz = advz::Advz<E, H>;
#[cfg(feature = "gpu-vid")]
/// Internal Jellyfish VID scheme
type Advz = advz::AdvzGPU<'static, E, H>;

/// Newtype wrapper for a VID scheme type that impls
/// [`VidScheme`], [`PayloadProver`], [`Precomputable`].
pub struct VidSchemeType(Advz);

/// Newtype wrapper for a large payload range proof.
///
/// Useful for namespace proofs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LargeRangeProofType(
    // # Type complexity
    //
    // Jellyfish's `LargeRangeProof` type has a prime field generic parameter `F`.
    // This `F` is determined by the type parameter `E` for `Advz`.
    // Jellyfish needs a more ergonomic way for downstream users to refer to this type.
    //
    // There is a `KzgEval` type alias in jellyfish that helps a little, but it's currently private:
    // <https://github.com/EspressoSystems/jellyfish/issues/423>
    // If it were public then we could instead use
    // `LargeRangeProof<KzgEval<E>>`
    // but that's still pretty crufty.
    LargeRangeProof<<UnivariateKzgPCS<E> as PolynomialCommitmentScheme>::Evaluation>,
);

/// Newtype wrapper for a small payload range proof.
///
/// Useful for transaction proofs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SmallRangeProofType(
    // # Type complexity
    //
    // Similar to the comments in `LargeRangeProofType`.
    SmallRangeProof<<UnivariateKzgPCS<E> as PolynomialCommitmentScheme>::Proof>,
);

lazy_static! {
    /// SRS comment
    ///
    /// TODO use a proper SRS
    /// https://github.com/EspressoSystems/HotShot/issues/1686
    static ref KZG_SRS: UnivariateUniversalParams<E> = {
        let mut rng = jf_utils::test_rng();
        UnivariateKzgPCS::<E>::gen_srs_for_testing(
            &mut rng,
            // TODO what's the maximum possible SRS size?
            checked_fft_size(200).unwrap(),
        )
        .unwrap()
    };
}

/// Private type alias for the EC pairing type parameter for [`Advz`].
type E = Bn254;
/// Private type alias for the hash type parameter for [`Advz`].
type H = Sha256;

// THE REST OF THIS FILE IS BOILERPLATE
//
// All this boilerplate can be deleted when we finally get
// type alias impl trait (TAIT):
// [rfcs/text/2515-type_alias_impl_trait.md at master · rust-lang/rfcs](https://github.com/rust-lang/rfcs/blob/master/text/2515-type_alias_impl_trait.md)
impl VidScheme for VidSchemeType {
    type Commit = <Advz as VidScheme>::Commit;
    type Share = <Advz as VidScheme>::Share;
    type Common = <Advz as VidScheme>::Common;

    fn commit_only<B>(&mut self, payload: B) -> VidResult<Self::Commit>
    where
        B: AsRef<[u8]>,
    {
        self.0.commit_only(payload)
    }

    fn disperse<B>(&mut self, payload: B) -> VidResult<VidDisperse<Self>>
    where
        B: AsRef<[u8]>,
    {
        self.0.disperse(payload).map(vid_disperse_conversion)
    }

    fn verify_share(
        &self,
        share: &Self::Share,
        common: &Self::Common,
        commit: &Self::Commit,
    ) -> VidResult<Result<(), ()>> {
        self.0.verify_share(share, common, commit)
    }

    fn recover_payload(&self, shares: &[Self::Share], common: &Self::Common) -> VidResult<Vec<u8>> {
        self.0.recover_payload(shares, common)
    }

    fn is_consistent(commit: &Self::Commit, common: &Self::Common) -> VidResult<()> {
        <Advz as VidScheme>::is_consistent(commit, common)
    }

    fn get_payload_byte_len(common: &Self::Common) -> usize {
        <Advz as VidScheme>::get_payload_byte_len(common)
    }

    fn get_num_storage_nodes(common: &Self::Common) -> usize {
        <Advz as VidScheme>::get_num_storage_nodes(common)
    }

    fn get_multiplicity(common: &Self::Common) -> usize {
        <Advz as VidScheme>::get_multiplicity(common)
    }
}

impl PayloadProver<LargeRangeProofType> for VidSchemeType {
    fn payload_proof<B>(&self, payload: B, range: Range<usize>) -> VidResult<LargeRangeProofType>
    where
        B: AsRef<[u8]>,
    {
        self.0
            .payload_proof(payload, range)
            .map(LargeRangeProofType)
    }

    fn payload_verify(
        &self,
        stmt: Statement<'_, Self>,
        proof: &LargeRangeProofType,
    ) -> VidResult<Result<(), ()>> {
        self.0.payload_verify(stmt_conversion(stmt), &proof.0)
    }
}

impl PayloadProver<SmallRangeProofType> for VidSchemeType {
    fn payload_proof<B>(&self, payload: B, range: Range<usize>) -> VidResult<SmallRangeProofType>
    where
        B: AsRef<[u8]>,
    {
        self.0
            .payload_proof(payload, range)
            .map(SmallRangeProofType)
    }

    fn payload_verify(
        &self,
        stmt: Statement<'_, Self>,
        proof: &SmallRangeProofType,
    ) -> VidResult<Result<(), ()>> {
        self.0.payload_verify(stmt_conversion(stmt), &proof.0)
    }
}

impl Precomputable for VidSchemeType {
    type PrecomputeData = <Advz as Precomputable>::PrecomputeData;

    fn commit_only_precompute<B>(
        &self,
        payload: B,
    ) -> VidResult<(Self::Commit, Self::PrecomputeData)>
    where
        B: AsRef<[u8]>,
    {
        self.0.commit_only_precompute(payload)
    }

    fn disperse_precompute<B>(
        &self,
        payload: B,
        data: &Self::PrecomputeData,
    ) -> VidResult<VidDisperse<Self>>
    where
        B: AsRef<[u8]>,
    {
        self.0
            .disperse_precompute(payload, data)
            .map(vid_disperse_conversion)
    }
}

/// Convert a [`VidDisperse<Advz>`] to a [`VidDisperse<VidSchemeType>`].
///
/// Foreign type rules prevent us from doing:
/// - `impl From<VidDisperse<VidSchemeType>> for VidDisperse<Advz>`
/// - `impl VidDisperse<VidSchemeType> {...}`
/// and similarly for `Statement`.
/// Thus, we accomplish type conversion via functions.
fn vid_disperse_conversion(vid_disperse: VidDisperse<Advz>) -> VidDisperse<VidSchemeType> {
    VidDisperse {
        shares: vid_disperse.shares,
        common: vid_disperse.common,
        commit: vid_disperse.commit,
    }
}

/// Convert a [`Statement<'_, VidSchemeType>`] to a [`Statement<'_, Advz>`].
fn stmt_conversion(stmt: Statement<'_, VidSchemeType>) -> Statement<'_, Advz> {
    Statement {
        payload_subslice: stmt.payload_subslice,
        range: stmt.range,
        commit: stmt.commit,
        common: stmt.common,
    }
}
