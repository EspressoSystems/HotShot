//! This module provides a dummy VID scheme implementation for test and benchmark.

use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use jf_vid::{precomputable::Precomputable, VidDisperse, VidResult, VidScheme};
use tagged_base64::tagged;

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
    VidSchemeType { num_storage_nodes }
}

/// Similar to [`vid_scheme()`], but with `KZG_SRS_TEST` for testing purpose only.
#[cfg(feature = "test-srs")]
pub fn vid_scheme_for_test(num_storage_nodes: usize) -> VidSchemeType {
    VidSchemeType { num_storage_nodes }
}

#[derive(CanonicalSerialize, CanonicalDeserialize, PartialEq, Eq, Clone, Hash, Debug, Copy)]
#[tagged("Dummy")]
/// Dummy type for test and benchmark only.
pub struct DummyType;

#[derive(CanonicalSerialize, CanonicalDeserialize, PartialEq, Eq, Clone, Hash, Debug)]
#[tagged("Dummy")]
/// Dummy type for test and benchmark only.
pub struct DummyShare(Vec<u8>);

impl AsRef<[u8; 32]> for DummyType {
    fn as_ref(&self) -> &[u8; 32] {
        &[0u8; 32]
    }
}

/// Dummy VID Scheme type for test and benchmark only, that doesn't do anything.
pub struct VidSchemeType {
    /// Number of storage nodes
    pub num_storage_nodes: usize,
}

impl VidScheme for VidSchemeType {
    type Commit = DummyType;
    type Share = DummyShare;
    type Common = DummyType;

    fn commit_only<B>(&mut self, _payload: B) -> VidResult<Self::Commit>
    where
        B: AsRef<[u8]>,
    {
        Ok(DummyType)
    }

    fn disperse<B>(&mut self, payload: B) -> VidResult<VidDisperse<Self>>
    where
        B: AsRef<[u8]>,
    {
        let share_len = payload.as_ref().len() * 2 / self.num_storage_nodes;
        Ok(VidDisperse {
            shares: vec![DummyShare(vec![0u8; share_len]); self.num_storage_nodes],
            common: DummyType,
            commit: DummyType,
        })
    }

    fn verify_share(
        &self,
        _share: &Self::Share,
        _common: &Self::Common,
        _commit: &Self::Commit,
    ) -> VidResult<Result<(), ()>> {
        Ok(Ok(()))
    }

    fn recover_payload(
        &self,
        _shares: &[Self::Share],
        _common: &Self::Common,
    ) -> VidResult<Vec<u8>> {
        Ok(vec![])
    }

    fn is_consistent(_commit: &Self::Commit, _common: &Self::Common) -> VidResult<()> {
        Ok(())
    }

    fn get_payload_byte_len(_common: &Self::Common) -> u32 {
        0
    }

    fn get_num_storage_nodes(_common: &Self::Common) -> u32 {
        0
    }

    fn get_multiplicity(_common: &Self::Common) -> u32 {
        0
    }
}

impl Precomputable for VidSchemeType {
    type PrecomputeData = DummyType;

    fn commit_only_precompute<B>(
        &self,
        _payload: B,
    ) -> VidResult<(Self::Commit, Self::PrecomputeData)>
    where
        B: AsRef<[u8]>,
    {
        Ok((DummyType, DummyType))
    }

    fn disperse_precompute<B>(
        &self,
        payload: B,
        _data: &Self::PrecomputeData,
    ) -> VidResult<VidDisperse<Self>>
    where
        B: AsRef<[u8]>,
    {
        let share_len = payload.as_ref().len() * 2 / self.num_storage_nodes;
        Ok(VidDisperse {
            shares: vec![DummyShare(vec![0u8; share_len]); self.num_storage_nodes],
            common: DummyType,
            commit: DummyType,
        })
    }
}
