//! The quorum certificate (QC) trait is a certificate of a sufficient quorum of distinct
//! parties voted for a message or statement.

use ark_std::{
    rand::{CryptoRng, RngCore},
    vec::Vec,
};
use bitvec::prelude::*;
use generic_array::{ArrayLength, GenericArray};
use jf_primitives::errors::PrimitivesError;
use jf_primitives::signatures::AggregateableSignatureSchemes;
use serde::{Deserialize, Serialize};

/// Trait for validating a QC built from different signatures on the same message
pub trait QuorumCertificate<A: AggregateableSignatureSchemes + Serialize + for<'a> Deserialize<'a>>
{
    /// Public parameters for generating the QC
    /// E.g: snark proving/verifying keys, list of (or pointer to) public keys stored in the smart contract.
    type QCProverParams: Serialize + for<'a> Deserialize<'a>;

    /// Public parameters for validating the QC
    /// E.g: verifying keys, stake table commitment
    type QCVerifierParams: Serialize + for<'a> Deserialize<'a>;

    /// Allows to fix the size of the message at compilation time.
    type MessageLength: ArrayLength<A::MessageUnit>;

    /// Type of the actual quorum certificate object
    type QC;

    /// Type of the quorum size (e.g. number of votes or accumulated weight of signatures)
    type QuorumSize;

    /// Produces a partial signature on a message with a single user signing key
    /// NOTE: the original message (vote) should be prefixed with the hash of the stake table.
    /// * `agg_sig_pp` - public parameters for aggregate signature
    /// * `message` - message to be signed
    /// * `sk` - user signing key
    /// * `returns` - a "simple" signature
    fn sign<R: CryptoRng + RngCore>(
        agg_sig_pp: &A::PublicParameter,
        message: &GenericArray<A::MessageUnit, Self::MessageLength>,
        sk: &A::SigningKey,
        prng: &mut R,
    ) -> Result<A::Signature, PrimitivesError>;

    /// Computes an aggregated signature from a set of partial signatures and the verification keys involved
    /// * `qc_pp` - public parameters for generating the QC
    /// * `signers` - a bool vector indicating the list of verification keys corresponding to the set of partial signatures
    /// * `sigs` - partial signatures on the same message
    /// * `returns` - an error if some of the partial signatures provided are invalid
    ///     or the number of partial signatures / verifications keys are different.
    ///     Otherwise return an obtained quorum certificate.
    fn assemble(
        qc_pp: &Self::QCProverParams,
        signers: &BitSlice,
        sigs: &[A::Signature],
    ) -> Result<Self::QC, PrimitivesError>;

    /// Checks an aggregated signature over some message provided as input
    /// * `qc_vp` - public parameters for validating the QC
    /// * `message` - message to check the aggregated signature against
    /// * `qc` - quroum certificate
    /// * `returns` - the quorum size if the qc is valid, an error otherwise.
    fn check(
        qc_vp: &Self::QCVerifierParams,
        message: &GenericArray<A::MessageUnit, Self::MessageLength>,
        qc: &Self::QC,
    ) -> Result<Self::QuorumSize, PrimitivesError>;

    /// Trace the list of signers given a qc.
    fn trace(
        qc_vp: &Self::QCVerifierParams,
        message: &GenericArray<A::MessageUnit, Self::MessageLength>,
        qc: &Self::QC,
    ) -> Result<Vec<A::VerificationKey>, PrimitivesError>;
}
