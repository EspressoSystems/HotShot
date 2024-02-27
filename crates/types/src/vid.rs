//! A constructor for [`VidScheme`].

use ark_bls12_381::Bls12_381;
use jf_primitives::{
    pcs::{prelude::UnivariateKzgPCS, PolynomialCommitmentScheme},
    vid::{
        advz::{
            payload_prover::{LargeRangeProof, SmallRangeProof},
            Advz,
        },
        payload_prover::{PayloadProver, Statement},
        precomputable::Precomputable,
        VidDisperse, VidResult, VidScheme,
    },
};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{fmt::Debug, ops::Range};

/// VID scheme constructor.
///
/// We prefer a return type of the form `impl VidScheme`.
/// But it's currently impossible to name an impl Trait return type:
/// [Naming impl trait in return types - Impl trait initiative](https://rust-lang.github.io/impl-trait-initiative/explainer/rpit_names.html)
/// For example: we cannot name any assoc types for `VidScheme` such as `Commit`, etc.
/// So instead we return a newtype that impls `VidScheme` via delegation.
/// # Panics
/// When the construction fails for the underlying VID scheme.
#[must_use]
pub fn vid_scheme(num_storage_nodes: usize) -> VidSchemeType {
    // TODO use a proper SRS
    // https://github.com/EspressoSystems/HotShot/issues/1686
    let srs = crate::data::test_srs(num_storage_nodes);

    // chunk_size is currently num_storage_nodes rounded down to a power of two
    // TODO chunk_size should be a function of the desired erasure code rate
    // https://github.com/EspressoSystems/HotShot/issues/2152
    let chunk_size = 1 << num_storage_nodes.ilog2();

    // TODO intelligent choice of multiplicity
    let multiplicity = 1;

    // TODO panic, return `Result`, or make `new` infallible upstream (eg. by panicking)?
    #[allow(clippy::panic)]
    VidSchemeType(Advz::new(chunk_size, num_storage_nodes, multiplicity, srs).unwrap_or_else(|err| panic!("advz construction failure:\n\t(num_storage nodes,chunk_size,multiplicity)=({num_storage_nodes},{chunk_size},{multiplicity})\n\terror: : {err}")))
}

// TODO can't name the return type of `vid_scheme`:
// https://rust-lang.github.io/impl-trait-initiative/explainer/rpit_names.html
// pub type VidCommitment = <<vid_scheme as FnOnce(usize)>::Output as VidScheme>::Commit;

/// Pairing type
type E = Bls12_381;
/// Hash type
type H = Sha256;

/// TODO doc
pub struct VidSchemeType(Advz<E, H>);

/// TODO doc
///
/// # Type complexity
///
/// Jellyfish's `LargeRangeProof` type has a prime field generic parameter `F`.
/// This `F` is determined by the pairing parameter for `Advz` currently returned by `test_vid_factory()`.
/// Jellyfish needs a more ergonomic way for downstream users to refer to this type.
///
/// There is a `KzgEval` type alias in jellyfish that helps a little, but it's currently private:
/// <https://github.com/EspressoSystems/jellyfish/issues/423>
/// If it were public then we could instead use
/// `LargeRangeProof<KzgEval<E>>`
/// but that's still pretty crufty.

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NamespaceProofType(
    LargeRangeProof<<UnivariateKzgPCS<E> as PolynomialCommitmentScheme>::Evaluation>,
);

/// TODO doc
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TransactionProofType(
    SmallRangeProof<<UnivariateKzgPCS<E> as PolynomialCommitmentScheme>::Proof>,
);

// BOILERPLATE FROM HERE TO THE END

impl VidScheme for VidSchemeType {
    type Commit = <Advz<E, H> as VidScheme>::Commit;
    type Share = <Advz<E, H> as VidScheme>::Share;
    type Common = <Advz<E, H> as VidScheme>::Common;

    fn commit_only<B>(&self, payload: B) -> VidResult<Self::Commit>
    where
        B: AsRef<[u8]>,
    {
        self.0.commit_only(payload)
    }

    fn disperse<B>(&self, payload: B) -> VidResult<VidDisperse<Self>>
    where
        B: AsRef<[u8]>,
    {
        self.0.disperse(payload).map(|res| VidDisperse {
            shares: res.shares,
            common: res.common,
            commit: res.commit,
        })
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
        <Advz<E, H> as VidScheme>::is_consistent(commit, common)
    }

    fn get_payload_byte_len(common: &Self::Common) -> usize {
        <Advz<E, H> as VidScheme>::get_payload_byte_len(common)
    }

    fn get_num_storage_nodes(common: &Self::Common) -> usize {
        <Advz<E, H> as VidScheme>::get_num_storage_nodes(common)
    }

    fn get_multiplicity(common: &Self::Common) -> usize {
        <Advz<E, H> as VidScheme>::get_multiplicity(common)
    }
}

impl PayloadProver<NamespaceProofType> for VidSchemeType {
    fn payload_proof<B>(&self, payload: B, range: Range<usize>) -> VidResult<NamespaceProofType>
    where
        B: AsRef<[u8]>,
    {
        self.0.payload_proof(payload, range).map(NamespaceProofType)
    }

    fn payload_verify(
        &self,
        stmt: Statement<'_, Self>,
        proof: &NamespaceProofType,
    ) -> VidResult<Result<(), ()>> {
        self.0.payload_verify(
            Statement {
                payload_subslice: stmt.payload_subslice,
                range: stmt.range,
                commit: stmt.commit,
                common: stmt.common,
            },
            &proof.0,
        )
    }
}

impl PayloadProver<TransactionProofType> for VidSchemeType {
    fn payload_proof<B>(&self, payload: B, range: Range<usize>) -> VidResult<TransactionProofType>
    where
        B: AsRef<[u8]>,
    {
        self.0
            .payload_proof(payload, range)
            .map(TransactionProofType)
    }

    fn payload_verify(
        &self,
        stmt: Statement<'_, Self>,
        proof: &TransactionProofType,
    ) -> VidResult<Result<(), ()>> {
        self.0.payload_verify(
            Statement {
                payload_subslice: stmt.payload_subslice,
                range: stmt.range,
                commit: stmt.commit,
                common: stmt.common,
            },
            &proof.0,
        )
    }
}

impl Precomputable for VidSchemeType {
    type PrecomputeData = <Advz<E, H> as Precomputable>::PrecomputeData;

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
            .map(|res| VidDisperse {
                shares: res.shares,
                common: res.common,
                commit: res.commit,
            })
    }
}

// Foreign trait rules prevent us from doing:
// `impl From<VidDisperse<VidSchemeType>> for VidDisperse<Advz<E, H>>`
// and similarly for `Statement`
