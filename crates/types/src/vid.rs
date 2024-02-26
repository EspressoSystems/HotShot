//! A constructor for [`VidScheme`].

use ark_bls12_381::Bls12_381;
use ark_ec::pairing::Pairing;
use jf_primitives::{
    merkle_tree::hasher::HasherDigest,
    pcs::{prelude::UnivariateKzgPCS, PolynomialCommitmentScheme},
    vid::{
        advz::{
            payload_prover::{LargeRangeProof, SmallRangeProof},
            Advz,
        },
        payload_prover::PayloadProver,
        precomputable::Precomputable,
    },
};
use serde::{de::DeserializeOwned, Serialize};
use sha2::Sha256;
use std::fmt::Debug;

/// VID scheme constructor.
///
/// We prefer a return type of the form `impl VidScheme`.
/// But it's currently impossible to name an impl Trait return type:
/// [Naming impl trait in return types - Impl trait initiative](https://rust-lang.github.io/impl-trait-initiative/explainer/rpit_names.html)
/// So instead we return a newtype that impls `VidScheme` via delegation.
pub fn vid_scheme(num_storage_nodes: usize) -> impl VidSchemeTrait {
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
    AdvzVidScheme::new(chunk_size, num_storage_nodes, multiplicity, srs).unwrap_or_else(|err| panic!("advz construction failure:\n\t(num_storage nodes,chunk_size,multiplicity)=({num_storage_nodes},{chunk_size},{multiplicity})\n\terror: : {err}"))
}

// TODO can't name the return type of `vid_scheme`:
// https://rust-lang.github.io/impl-trait-initiative/explainer/rpit_names.html
// pub type VidCommitment = <<vid_scheme as FnOnce(usize)>::Output as VidScheme>::Commit;

// Private type alias for `Advz`.
// Set curve/pairing and hash function here.
type AdvzVidScheme = Advz<Bls12_381, Sha256>;

/// wrapper trait workaround for nested impl Trait
pub trait VidSchemeTrait:
    PayloadProver<Self::NamespaceProof> + PayloadProver<Self::TransactionProof> + Precomputable
{
    /// Namespace proof
    type NamespaceProof: Clone + Debug + Eq + PartialEq + Serialize + DeserializeOwned;
    /// Transaction proof
    type TransactionProof: Clone + Debug + Eq + PartialEq + Serialize + DeserializeOwned;
}

impl<E, H> VidSchemeTrait for Advz<E, H>
where
    E: Pairing,
    H: HasherDigest,
{
    /// # Type complexity
    ///
    /// Jellyfish's `LargeRangeProof` type has a prime field generic parameter `F`.
    /// This `F` is determined by the pairing parameter for `Advz` currently returned by `test_vid_factory()`.
    /// Jellyfish needs a more ergonomic way for downstream users to refer to this type.
    ///
    /// There is a `KzgEval` type alias in jellyfish that helps a little, but it's currently private:
    /// https://github.com/EspressoSystems/jellyfish/issues/423
    /// If it were public then we could instead use
    /// `LargeRangeProof<KzgEval<E>>`
    /// but that's still pretty crufty.
    type NamespaceProof =
        LargeRangeProof<<UnivariateKzgPCS<E> as PolynomialCommitmentScheme>::Evaluation>;

    type TransactionProof =
        SmallRangeProof<<UnivariateKzgPCS<E> as PolynomialCommitmentScheme>::Proof>;
}
