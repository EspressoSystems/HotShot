//! Vote and vote accumulator types
//!
//! This module contains types used to represent the various types of votes that `HotShot` nodes
//! can send, and vote accumulator that converts votes into certificates.

use crate::{
    certificate::{AssembledSignature, QuorumCertificate},
    traits::{
        election::{VoteData, VoteToken},
        node_implementation::NodeType,
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
        BlockPayload,
    },
};
use bincode::Options;
use bitvec::prelude::*;
use commit::{Commitment, CommitmentBounds, Committable};
use either::Either;
use ethereum_types::U256;
use hotshot_utils::bincode::bincode_opts;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    hash::Hash,
    marker::PhantomData,
    num::NonZeroU64,
};
use tracing::error;

/// The vote sent by consensus messages.
pub trait VoteType<TYPES: NodeType, COMMITMENT: CommitmentBounds>:
    Debug + Clone + 'static + Serialize + for<'a> Deserialize<'a> + Send + Sync + PartialEq
{
    /// Get the view this vote was cast for
    fn get_view(&self) -> TYPES::Time;
    /// Get the signature key associated with this vote
    fn get_key(&self) -> TYPES::SignatureKey;
    /// Get the signature associated with this vote
    fn get_signature(&self) -> EncodedSignature;
    /// Get the data this vote was signed over
    fn get_data(&self) -> VoteData<COMMITMENT>;
    /// Get the vote token of this vote
    fn get_vote_token(&self) -> TYPES::VoteTokenType;
}

/// A vote on DA proposal.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
#[serde(bound(deserialize = ""))]
pub struct DAVote<TYPES: NodeType> {
    /// The signature share associated with this vote
    pub signature: (EncodedPublicKey, EncodedSignature),
    /// The block payload commitment being voted on.
    pub payload_commitment: Commitment<BlockPayload<TYPES::Transaction>>,
    /// The view this vote was cast for
    pub current_view: TYPES::Time,
    /// The vote token generated by this replica
    pub vote_token: TYPES::VoteTokenType,
    /// The vote data this vote is signed over
    pub vote_data: VoteData<Commitment<BlockPayload<TYPES::Transaction>>>,
}

/// A vote on VID proposal.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
#[serde(bound(deserialize = ""))]
pub struct VIDVote<TYPES: NodeType> {
    /// The signature share associated with this vote
    pub signature: (EncodedPublicKey, EncodedSignature),
    /// The block payload commitment being voted on.
    pub payload_commitment: Commitment<BlockPayload<TYPES::Transaction>>,
    /// The view this vote was cast for
    pub current_view: TYPES::Time,
    /// The vote token generated by this replica
    pub vote_token: TYPES::VoteTokenType,
    /// The vote data this vote is signed over
    pub vote_data: VoteData<Commitment<BlockPayload<TYPES::Transaction>>>,
}

/// A positive or negative vote on validating or commitment proposal.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(bound(deserialize = ""))]
pub struct YesOrNoVote<TYPES: NodeType, COMMITMENT: CommitmentBounds> {
    /// TODO we should remove this
    /// this is correct, but highly inefficient
    /// we should check a cache, and if that fails request the qc
    pub justify_qc_commitment: Commitment<QuorumCertificate<TYPES, COMMITMENT>>,
    /// The signature share associated with this vote
    pub signature: (EncodedPublicKey, EncodedSignature),
    /// The leaf commitment being voted on.
    pub leaf_commitment: COMMITMENT,
    /// The view this vote was cast for
    pub current_view: TYPES::Time,
    /// The vote token generated by this replica
    pub vote_token: TYPES::VoteTokenType,
    /// The vote data this vote is signed over
    pub vote_data: VoteData<COMMITMENT>,
}

/// A timeout vote
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(bound(deserialize = ""))]
pub struct TimeoutVote<TYPES: NodeType> {
    /// The signature share associated with this vote
    pub signature: (EncodedPublicKey, EncodedSignature),
    /// The view this vote was cast for
    pub current_view: TYPES::Time,
    /// The vote token generated by this replica
    pub vote_token: TYPES::VoteTokenType,
}

impl<TYPES: NodeType> VoteType<TYPES, Commitment<TYPES::Time>> for TimeoutVote<TYPES> {
    fn get_view(&self) -> <TYPES as NodeType>::Time {
        self.current_view
    }

    fn get_key(&self) -> <TYPES as NodeType>::SignatureKey {
        <TYPES::SignatureKey as SignatureKey>::from_bytes(&self.signature.0).unwrap()
    }

    fn get_signature(&self) -> EncodedSignature {
        self.signature.1.clone()
    }

    fn get_data(&self) -> VoteData<Commitment<TYPES::Time>> {
        VoteData::Timeout(self.get_view().commit())
    }

    fn get_vote_token(&self) -> <TYPES as NodeType>::VoteTokenType {
        self.vote_token.clone()
    }
}

/// The internals of a view sync vote
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(bound(deserialize = ""))]
pub struct ViewSyncVoteInternal<TYPES: NodeType> {
    /// The public key associated with the relay.
    pub relay_pub_key: EncodedPublicKey,
    /// The relay this vote is intended for
    pub relay: u64,
    /// The view number we are trying to sync on
    pub round: TYPES::Time,
    /// This node's signature over the VoteData
    pub signature: (EncodedPublicKey, EncodedSignature),
    /// The vote token generated by this replica
    pub vote_token: TYPES::VoteTokenType,
    /// The vote data this vote is signed over
    pub vote_data: VoteData<Commitment<ViewSyncData<TYPES>>>,
}

/// The data View Sync votes are signed over
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Hash, Eq)]
#[serde(bound(deserialize = ""))]
pub struct ViewSyncData<TYPES: NodeType> {
    /// The relay this vote is intended for
    pub relay: EncodedPublicKey,
    /// The view number we are trying to sync on
    pub round: TYPES::Time,
}

impl<TYPES: NodeType> Committable for ViewSyncData<TYPES> {
    fn commit(&self) -> Commitment<Self> {
        let builder = commit::RawCommitmentBuilder::new("Quorum Certificate Commitment");

        builder
            .var_size_field("Relay public key", &self.relay.0)
            .u64(*self.round)
            .finalize()
    }
}

/// Votes to synchronize the network on a single view
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(bound(deserialize = ""))]
pub enum ViewSyncVote<TYPES: NodeType> {
    /// PreCommit vote
    PreCommit(ViewSyncVoteInternal<TYPES>),
    /// Commit vote
    Commit(ViewSyncVoteInternal<TYPES>),
    /// Finalize vote
    Finalize(ViewSyncVoteInternal<TYPES>),
}

impl<TYPES: NodeType> ViewSyncVote<TYPES> {
    /// Get the encoded signature.
    pub fn signature(&self) -> EncodedSignature {
        match &self {
            ViewSyncVote::PreCommit(vote_internal)
            | ViewSyncVote::Commit(vote_internal)
            | ViewSyncVote::Finalize(vote_internal) => vote_internal.signature.1.clone(),
        }
    }
    /// Get the signature key.
    /// # Panics
    /// If the deserialization fails.
    pub fn signature_key(&self) -> TYPES::SignatureKey {
        let encoded = match &self {
            ViewSyncVote::PreCommit(vote_internal)
            | ViewSyncVote::Commit(vote_internal)
            | ViewSyncVote::Finalize(vote_internal) => vote_internal.signature.0.clone(),
        };
        <TYPES::SignatureKey as SignatureKey>::from_bytes(&encoded).unwrap()
    }
    /// Get the relay.
    pub fn relay(&self) -> u64 {
        match &self {
            ViewSyncVote::PreCommit(vote_internal)
            | ViewSyncVote::Commit(vote_internal)
            | ViewSyncVote::Finalize(vote_internal) => vote_internal.relay,
        }
    }
    /// Get the round number.
    pub fn round(&self) -> TYPES::Time {
        match &self {
            ViewSyncVote::PreCommit(vote_internal)
            | ViewSyncVote::Commit(vote_internal)
            | ViewSyncVote::Finalize(vote_internal) => vote_internal.round,
        }
    }
}

/// Votes on validating or commitment proposal.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(bound(deserialize = ""))]
pub enum QuorumVote<TYPES: NodeType, COMMITMENT: CommitmentBounds> {
    /// Posivite vote.
    Yes(YesOrNoVote<TYPES, COMMITMENT>),
    /// Negative vote.
    No(YesOrNoVote<TYPES, COMMITMENT>),
}

impl<TYPES: NodeType> VoteType<TYPES, Commitment<BlockPayload<TYPES::Transaction>>>
    for DAVote<TYPES>
{
    fn get_view(&self) -> TYPES::Time {
        self.current_view
    }
    fn get_key(&self) -> <TYPES as NodeType>::SignatureKey {
        self.signature_key()
    }
    fn get_signature(&self) -> EncodedSignature {
        self.signature.1.clone()
    }
    fn get_data(&self) -> VoteData<Commitment<BlockPayload<TYPES::Transaction>>> {
        self.vote_data.clone()
    }
    fn get_vote_token(&self) -> <TYPES as NodeType>::VoteTokenType {
        self.vote_token.clone()
    }
}

impl<TYPES: NodeType> DAVote<TYPES> {
    /// Get the signature key.
    /// # Panics
    /// If the deserialization fails.
    pub fn signature_key(&self) -> TYPES::SignatureKey {
        <TYPES::SignatureKey as SignatureKey>::from_bytes(&self.signature.0).unwrap()
    }
}

impl<TYPES: NodeType> VoteType<TYPES, Commitment<BlockPayload<TYPES::Transaction>>>
    for VIDVote<TYPES>
{
    fn get_view(&self) -> TYPES::Time {
        self.current_view
    }
    fn get_key(&self) -> <TYPES as NodeType>::SignatureKey {
        self.signature_key()
    }
    fn get_signature(&self) -> EncodedSignature {
        self.signature.1.clone()
    }
    fn get_data(&self) -> VoteData<Commitment<BlockPayload<TYPES::Transaction>>> {
        self.vote_data.clone()
    }
    fn get_vote_token(&self) -> <TYPES as NodeType>::VoteTokenType {
        self.vote_token.clone()
    }
}

impl<TYPES: NodeType> VIDVote<TYPES> {
    /// Get the signature key.
    /// # Panics
    /// If the deserialization fails.
    pub fn signature_key(&self) -> TYPES::SignatureKey {
        <TYPES::SignatureKey as SignatureKey>::from_bytes(&self.signature.0).unwrap()
    }
}

impl<TYPES: NodeType, COMMITMENT: CommitmentBounds> VoteType<TYPES, COMMITMENT>
    for QuorumVote<TYPES, COMMITMENT>
{
    fn get_view(&self) -> TYPES::Time {
        match self {
            QuorumVote::Yes(v) | QuorumVote::No(v) => v.current_view,
        }
    }

    fn get_key(&self) -> <TYPES as NodeType>::SignatureKey {
        self.signature_key()
    }
    fn get_signature(&self) -> EncodedSignature {
        self.signature()
    }
    fn get_data(&self) -> VoteData<COMMITMENT> {
        match self {
            QuorumVote::Yes(v) | QuorumVote::No(v) => v.vote_data.clone(),
        }
    }
    fn get_vote_token(&self) -> <TYPES as NodeType>::VoteTokenType {
        match self {
            QuorumVote::Yes(v) | QuorumVote::No(v) => v.vote_token.clone(),
        }
    }
}

impl<TYPES: NodeType, COMMITMENT: CommitmentBounds> QuorumVote<TYPES, COMMITMENT> {
    /// Get the encoded signature.

    pub fn signature(&self) -> EncodedSignature {
        match &self {
            Self::Yes(vote) | Self::No(vote) => vote.signature.1.clone(),
        }
    }
    /// Get the signature key.
    /// # Panics
    /// If the deserialization fails.
    pub fn signature_key(&self) -> TYPES::SignatureKey {
        let encoded = match &self {
            Self::Yes(vote) | Self::No(vote) => vote.signature.0.clone(),
        };
        <TYPES::SignatureKey as SignatureKey>::from_bytes(&encoded).unwrap()
    }
}

impl<TYPES: NodeType> VoteType<TYPES, Commitment<ViewSyncData<TYPES>>> for ViewSyncVote<TYPES> {
    fn get_view(&self) -> TYPES::Time {
        match self {
            ViewSyncVote::PreCommit(v) | ViewSyncVote::Commit(v) | ViewSyncVote::Finalize(v) => {
                v.round
            }
        }
    }
    fn get_key(&self) -> <TYPES as NodeType>::SignatureKey {
        self.signature_key()
    }

    fn get_signature(&self) -> EncodedSignature {
        self.signature()
    }
    fn get_data(&self) -> VoteData<Commitment<ViewSyncData<TYPES>>> {
        match self {
            ViewSyncVote::PreCommit(vote_internal)
            | ViewSyncVote::Commit(vote_internal)
            | ViewSyncVote::Finalize(vote_internal) => vote_internal.vote_data.clone(),
        }
    }

    fn get_vote_token(&self) -> <TYPES as NodeType>::VoteTokenType {
        match self {
            ViewSyncVote::PreCommit(vote_internal)
            | ViewSyncVote::Commit(vote_internal)
            | ViewSyncVote::Finalize(vote_internal) => vote_internal.vote_token.clone(),
        }
    }
}

/// Accumulator trait used to accumulate votes into an `AssembledSignature`
pub trait Accumulator<
    TYPES: NodeType,
    COMMITMENT: CommitmentBounds,
    VOTE: VoteType<TYPES, COMMITMENT>,
>: Sized
{
    /// Append 1 vote to the accumulator.  If the threshold is not reached, return
    /// the accumulator, else return the `AssembledSignature`
    /// Only called from inside `accumulate_internal`
    fn append(
        self,
        vote: VOTE,
        vote_node_id: usize,
        stake_table_entries: Vec<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry>,
    ) -> Either<Self, AssembledSignature<TYPES>>;
}

// TODO Make a default accumulator
// https://github.com/EspressoSystems/HotShot/issues/1797
/// Accumulator for `TimeoutVote`s
pub struct TimeoutVoteAccumulator<
    TYPES: NodeType,
    COMMITMENT: CommitmentBounds,
    VOTE: VoteType<TYPES, COMMITMENT>,
> {
    /// Map of all da signatures accumlated so far
    pub da_vote_outcomes: VoteMap<COMMITMENT, TYPES::VoteTokenType>,
    /// A quorum's worth of stake, generally 2f + 1
    pub success_threshold: NonZeroU64,
    /// A list of valid signatures for certificate aggregation
    pub sig_lists: Vec<<TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType>,
    /// A bitvec to indicate which node is active and send out a valid signature for certificate aggregation, this automatically do uniqueness check
    pub signers: BitVec,
    /// Phantom data to specify the vote this accumulator is for
    pub phantom: PhantomData<VOTE>,
}

impl<
        TYPES: NodeType,
        COMMITMENT: CommitmentBounds + Clone + Copy + PartialEq + Eq + Hash,
        VOTE: VoteType<TYPES, COMMITMENT>,
    > Accumulator<TYPES, COMMITMENT, VOTE> for DAVoteAccumulator<TYPES, COMMITMENT, VOTE>
{
    fn append(
        mut self,
        vote: VOTE,
        vote_node_id: usize,
        stake_table_entries: Vec<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry>,
    ) -> Either<Self, AssembledSignature<TYPES>> {
        let VoteData::DA(vote_commitment) = vote.get_data() else {
            return Either::Left(self);
        };

        let encoded_key = vote.get_key().to_bytes();

        // Deserialize the signature so that it can be assembeld into a QC
        // TODO ED Update this once we've gotten rid of EncodedSignature
        let original_signature: <TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType =
            bincode_opts()
                .deserialize(&vote.get_signature().0)
                .expect("Deserialization on the signature shouldn't be able to fail.");

        let (da_stake_casted, da_vote_map) = self
            .da_vote_outcomes
            .entry(vote_commitment)
            .or_insert_with(|| (0, BTreeMap::new()));

        // Check for duplicate vote
        // TODO ED Re-encoding signature key to bytes until we get rid of EncodedKey
        // Have to do this because SignatureKey is not hashable
        if da_vote_map.contains_key(&encoded_key) {
            return Either::Left(self);
        }

        if self.signers.get(vote_node_id).as_deref() == Some(&true) {
            error!("Node id is already in signers list");
            return Either::Left(self);
        }
        self.signers.set(vote_node_id, true);
        self.sig_lists.push(original_signature);

        // Already checked that vote data was for a DA vote above
        *da_stake_casted += u64::from(vote.get_vote_token().vote_count());
        da_vote_map.insert(
            encoded_key,
            (vote.get_signature(), vote.get_data(), vote.get_vote_token()),
        );

        if *da_stake_casted >= u64::from(self.success_threshold) {
            // Assemble QC
            let real_qc_pp = <TYPES::SignatureKey as SignatureKey>::get_public_parameter(
                stake_table_entries.clone(),
                U256::from(self.success_threshold.get()),
            );

            let real_qc_sig = <TYPES::SignatureKey as SignatureKey>::assemble(
                &real_qc_pp,
                self.signers.as_bitslice(),
                &self.sig_lists[..],
            );

            self.da_vote_outcomes.remove(&vote_commitment);

            return Either::Right(AssembledSignature::DA(real_qc_sig));
        }
        Either::Left(self)
    }
}

impl<
        TYPES: NodeType,
        COMMITMENT: CommitmentBounds + Clone + Copy + PartialEq + Eq + Hash,
        VOTE: VoteType<TYPES, COMMITMENT>,
    > Accumulator<TYPES, COMMITMENT, VOTE> for TimeoutVoteAccumulator<TYPES, COMMITMENT, VOTE>
{
    fn append(
        mut self,
        vote: VOTE,
        vote_node_id: usize,
        stake_table_entries: Vec<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry>,
    ) -> Either<Self, AssembledSignature<TYPES>> {
        let VoteData::Timeout(vote_commitment) = vote.get_data() else {
            return Either::Left(self);
        };

        let encoded_key = vote.get_key().to_bytes();

        // Deserialize the signature so that it can be assembeld into a QC
        // TODO ED Update this once we've gotten rid of EncodedSignature
        let original_signature: <TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType =
            bincode_opts()
                .deserialize(&vote.get_signature().0)
                .expect("Deserialization on the signature shouldn't be able to fail.");

        let (da_stake_casted, da_vote_map) = self
            .da_vote_outcomes
            .entry(vote_commitment)
            .or_insert_with(|| (0, BTreeMap::new()));

        // Check for duplicate vote
        // TODO ED Re-encoding signature key to bytes until we get rid of EncodedKey
        // Have to do this because SignatureKey is not hashable
        if da_vote_map.contains_key(&encoded_key) {
            return Either::Left(self);
        }

        if self.signers.get(vote_node_id).as_deref() == Some(&true) {
            error!("Node id is already in signers list");
            return Either::Left(self);
        }
        self.signers.set(vote_node_id, true);
        self.sig_lists.push(original_signature);

        // Already checked that vote data was for a DA vote above
        *da_stake_casted += u64::from(vote.get_vote_token().vote_count());
        da_vote_map.insert(
            encoded_key,
            (vote.get_signature(), vote.get_data(), vote.get_vote_token()),
        );

        if *da_stake_casted >= u64::from(self.success_threshold) {
            // Assemble QC
            let real_qc_pp = <TYPES::SignatureKey as SignatureKey>::get_public_parameter(
                stake_table_entries.clone(),
                U256::from(self.success_threshold.get()),
            );

            let real_qc_sig = <TYPES::SignatureKey as SignatureKey>::assemble(
                &real_qc_pp,
                self.signers.as_bitslice(),
                &self.sig_lists[..],
            );

            self.da_vote_outcomes.remove(&vote_commitment);

            return Either::Right(AssembledSignature::Timeout(real_qc_sig));
        }
        Either::Left(self)
    }
}

impl<
        TYPES: NodeType,
        COMMITMENT: CommitmentBounds + Clone + Copy + PartialEq + Eq + Hash,
        VOTE: VoteType<TYPES, COMMITMENT>,
    > Accumulator<TYPES, COMMITMENT, VOTE> for VIDVoteAccumulator<TYPES, COMMITMENT, VOTE>
{
    fn append(
        mut self,
        vote: VOTE,
        vote_node_id: usize,
        stake_table_entries: Vec<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry>,
    ) -> Either<Self, AssembledSignature<TYPES>> {
        let VoteData::VID(vote_commitment) = vote.get_data() else {
            return Either::Left(self);
        };

        let encoded_key = vote.get_key().to_bytes();

        // Deserialize the signature so that it can be assembeld into a QC
        // TODO ED Update this once we've gotten rid of EncodedSignature
        let original_signature: <TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType =
            bincode_opts()
                .deserialize(&vote.get_signature().0)
                .expect("Deserialization on the signature shouldn't be able to fail.");

        let (vid_stake_casted, vid_vote_map) = self
            .vid_vote_outcomes
            .entry(vote_commitment)
            .or_insert_with(|| (0, BTreeMap::new()));

        // Check for duplicate vote
        // TODO ED Re-encoding signature key to bytes until we get rid of EncodedKey
        // Have to do this because SignatureKey is not hashable
        if vid_vote_map.contains_key(&encoded_key) {
            return Either::Left(self);
        }

        if self.signers.get(vote_node_id).as_deref() == Some(&true) {
            error!("Node id is already in signers list");
            return Either::Left(self);
        }
        self.signers.set(vote_node_id, true);
        self.sig_lists.push(original_signature);

        // Already checked that vote data was for a VID vote above
        *vid_stake_casted += u64::from(vote.get_vote_token().vote_count());
        vid_vote_map.insert(
            encoded_key,
            (vote.get_signature(), vote.get_data(), vote.get_vote_token()),
        );

        if *vid_stake_casted >= u64::from(self.success_threshold) {
            // Assemble QC
            let real_qc_pp = <TYPES::SignatureKey as SignatureKey>::get_public_parameter(
                stake_table_entries.clone(),
                U256::from(self.success_threshold.get()),
            );

            let real_qc_sig = <TYPES::SignatureKey as SignatureKey>::assemble(
                &real_qc_pp,
                self.signers.as_bitslice(),
                &self.sig_lists[..],
            );

            self.vid_vote_outcomes.remove(&vote_commitment);

            return Either::Right(AssembledSignature::VID(real_qc_sig));
        }
        Either::Left(self)
    }
}

/// Accumulates DA votes
pub struct DAVoteAccumulator<
    TYPES: NodeType,
    COMMITMENT: CommitmentBounds + Clone,
    VOTE: VoteType<TYPES, COMMITMENT>,
> {
    /// Map of all da signatures accumlated so far
    pub da_vote_outcomes: VoteMap<COMMITMENT, TYPES::VoteTokenType>,
    /// A quorum's worth of stake, generally 2f + 1
    pub success_threshold: NonZeroU64,
    /// A list of valid signatures for certificate aggregation
    pub sig_lists: Vec<<TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType>,
    /// A bitvec to indicate which node is active and send out a valid signature for certificate aggregation, this automatically do uniqueness check
    pub signers: BitVec,
    /// Phantom data to specify the vote this accumulator is for
    pub phantom: PhantomData<VOTE>,
}

/// Accumulates VID votes
pub struct VIDVoteAccumulator<
    TYPES: NodeType,
    COMMITMENT: CommitmentBounds + Clone,
    VOTE: VoteType<TYPES, COMMITMENT>,
> {
    /// Map of all VID signatures accumlated so far
    pub vid_vote_outcomes: VoteMap<COMMITMENT, TYPES::VoteTokenType>,
    /// A quorum's worth of stake, generally 2f + 1
    pub success_threshold: NonZeroU64,
    /// A list of valid signatures for certificate aggregation
    pub sig_lists: Vec<<TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType>,
    /// A bitvec to indicate which node is active and send out a valid signature for certificate aggregation, this automatically do uniqueness check
    pub signers: BitVec,
    /// Phantom data to specify the vote this accumulator is for
    pub phantom: PhantomData<VOTE>,
}

/// Accumulate quorum votes
pub struct QuorumVoteAccumulator<
    TYPES: NodeType,
    COMMITMENT: CommitmentBounds,
    VOTE: VoteType<TYPES, COMMITMENT>,
> {
    /// Map of all signatures accumlated so far
    pub total_vote_outcomes: VoteMap<COMMITMENT, TYPES::VoteTokenType>,
    /// Map of all yes signatures accumlated so far
    pub yes_vote_outcomes: VoteMap<COMMITMENT, TYPES::VoteTokenType>,
    /// Map of all no signatures accumlated so far
    pub no_vote_outcomes: VoteMap<COMMITMENT, TYPES::VoteTokenType>,

    /// A quorum's worth of stake, generally 2f + 1
    pub success_threshold: NonZeroU64,
    /// A failure threshold, generally f + 1
    pub failure_threshold: NonZeroU64,
    /// A list of valid signatures for certificate aggregation
    pub sig_lists: Vec<<TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType>,
    /// A bitvec to indicate which node is active and send out a valid signature for certificate aggregation, this automatically do uniqueness check
    pub signers: BitVec,
    /// Phantom data to ensure this struct is over a specific `VoteType` implementation
    pub phantom: PhantomData<VOTE>,
}

impl<
        TYPES: NodeType,
        COMMITMENT: CommitmentBounds + Clone + Copy + PartialEq + Eq + Hash,
        VOTE: VoteType<TYPES, COMMITMENT>,
    > Accumulator<TYPES, COMMITMENT, VOTE> for QuorumVoteAccumulator<TYPES, COMMITMENT, VOTE>
{
    fn append(
        mut self,
        vote: VOTE,
        vote_node_id: usize,
        stake_table_entries: Vec<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry>,
    ) -> Either<Self, AssembledSignature<TYPES>> {
        let (VoteData::Yes(vote_commitment) | VoteData::No(vote_commitment)) = vote.get_data()
        else {
            return Either::Left(self);
        };

        let encoded_key = vote.get_key().to_bytes();

        // Deserialize the signature so that it can be assembeld into a QC
        // TODO ED Update this once we've gotten rid of EncodedSignature
        let original_signature: <TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType =
            bincode_opts()
                .deserialize(&vote.get_signature().0)
                .expect("Deserialization on the signature shouldn't be able to fail.");

        let (total_stake_casted, total_vote_map) = self
            .total_vote_outcomes
            .entry(vote_commitment)
            .or_insert_with(|| (0, BTreeMap::new()));

        let (yes_stake_casted, yes_vote_map) = self
            .yes_vote_outcomes
            .entry(vote_commitment)
            .or_insert_with(|| (0, BTreeMap::new()));

        let (no_stake_casted, no_vote_map) = self
            .no_vote_outcomes
            .entry(vote_commitment)
            .or_insert_with(|| (0, BTreeMap::new()));

        // Check for duplicate vote
        // TODO ED Re-encoding signature key to bytes until we get rid of EncodedKey
        // Have to do this because SignatureKey is not hashable
        if total_vote_map.contains_key(&encoded_key) {
            return Either::Left(self);
        }

        if self.signers.get(vote_node_id).as_deref() == Some(&true) {
            error!("Node id is already in signers list");
            return Either::Left(self);
        }
        self.signers.set(vote_node_id, true);
        self.sig_lists.push(original_signature);

        *total_stake_casted += u64::from(vote.get_vote_token().vote_count());
        total_vote_map.insert(
            encoded_key.clone(),
            (vote.get_signature(), vote.get_data(), vote.get_vote_token()),
        );

        match vote.get_data() {
            VoteData::Yes(_) => {
                *yes_stake_casted += u64::from(vote.get_vote_token().vote_count());
                yes_vote_map.insert(
                    encoded_key,
                    (vote.get_signature(), vote.get_data(), vote.get_vote_token()),
                );
            }
            VoteData::No(_) => {
                *no_stake_casted += u64::from(vote.get_vote_token().vote_count());
                no_vote_map.insert(
                    encoded_key,
                    (vote.get_signature(), vote.get_data(), vote.get_vote_token()),
                );
            }
            _ => return Either::Left(self),
        }

        if *total_stake_casted >= u64::from(self.success_threshold) {
            // Assemble QC
            let real_qc_pp = <TYPES::SignatureKey as SignatureKey>::get_public_parameter(
                stake_table_entries.clone(),
                U256::from(self.success_threshold.get()),
            );

            let real_qc_sig = <TYPES::SignatureKey as SignatureKey>::assemble(
                &real_qc_pp,
                self.signers.as_bitslice(),
                &self.sig_lists[..],
            );

            if *yes_stake_casted >= u64::from(self.success_threshold) {
                self.yes_vote_outcomes.remove(&vote_commitment);
                return Either::Right(AssembledSignature::Yes(real_qc_sig));
            } else if *no_stake_casted >= u64::from(self.failure_threshold) {
                self.total_vote_outcomes.remove(&vote_commitment);
                return Either::Right(AssembledSignature::No(real_qc_sig));
            }
        }
        Either::Left(self)
    }
}

/// Accumulates view sync votes
pub struct ViewSyncVoteAccumulator<
    TYPES: NodeType,
    COMMITMENT: CommitmentBounds,
    VOTE: VoteType<TYPES, COMMITMENT>,
> {
    /// Map of all pre_commit signatures accumlated so far
    pub pre_commit_vote_outcomes: VoteMap<COMMITMENT, TYPES::VoteTokenType>,
    /// Map of all ommit signatures accumlated so far
    pub commit_vote_outcomes: VoteMap<COMMITMENT, TYPES::VoteTokenType>,
    /// Map of all finalize signatures accumlated so far
    pub finalize_vote_outcomes: VoteMap<COMMITMENT, TYPES::VoteTokenType>,

    /// A quorum's worth of stake, generally 2f + 1
    pub success_threshold: NonZeroU64,
    /// A quorum's failure threshold, generally f + 1
    pub failure_threshold: NonZeroU64,
    /// A list of valid signatures for certificate aggregation
    pub sig_lists: Vec<<TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType>,
    /// A bitvec to indicate which node is active and send out a valid signature for certificate aggregation, this automatically do uniqueness check
    pub signers: BitVec,
    /// Phantom data since we want the accumulator to be attached to a single `VoteType`  
    pub phantom: PhantomData<VOTE>,
}

impl<
        TYPES: NodeType,
        COMMITMENT: CommitmentBounds + Clone + Copy + PartialEq + Eq + Hash,
        VOTE: VoteType<TYPES, COMMITMENT>,
    > Accumulator<TYPES, COMMITMENT, VOTE> for ViewSyncVoteAccumulator<TYPES, COMMITMENT, VOTE>
{
    #[allow(clippy::too_many_lines)]
    fn append(
        mut self,
        vote: VOTE,
        vote_node_id: usize,
        stake_table_entries: Vec<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry>,
    ) -> Either<Self, AssembledSignature<TYPES>> {
        let (VoteData::ViewSyncPreCommit(vote_commitment)
        | VoteData::ViewSyncCommit(vote_commitment)
        | VoteData::ViewSyncFinalize(vote_commitment)) = vote.get_data()
        else {
            return Either::Left(self);
        };

        // error!("Vote is {:?}", vote.clone());

        let encoded_key = vote.get_key().to_bytes();

        // Deserialize the signature so that it can be assembeld into a QC
        // TODO ED Update this once we've gotten rid of EncodedSignature
        let original_signature: <TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType =
            bincode_opts()
                .deserialize(&vote.get_signature().0)
                .expect("Deserialization on the signature shouldn't be able to fail.");

        let (pre_commit_stake_casted, pre_commit_vote_map) = self
            .pre_commit_vote_outcomes
            .entry(vote_commitment)
            .or_insert_with(|| (0, BTreeMap::new()));

        // Check for duplicate vote
        if pre_commit_vote_map.contains_key(&encoded_key) {
            return Either::Left(self);
        }

        let (commit_stake_casted, commit_vote_map) = self
            .commit_vote_outcomes
            .entry(vote_commitment)
            .or_insert_with(|| (0, BTreeMap::new()));

        if commit_vote_map.contains_key(&encoded_key) {
            return Either::Left(self);
        }

        let (finalize_stake_casted, finalize_vote_map) = self
            .finalize_vote_outcomes
            .entry(vote_commitment)
            .or_insert_with(|| (0, BTreeMap::new()));

        if finalize_vote_map.contains_key(&encoded_key) {
            return Either::Left(self);
        }

        // update the active_keys and sig_lists
        // TODO ED Possible bug where a node sends precommit vote and then commit vote after
        // precommit cert is formed, their commit vote won't be counted because of this check
        // Probably need separate signers vecs.
        if self.signers.get(vote_node_id).as_deref() == Some(&true) {
            error!("node id already in signers");
            return Either::Left(self);
        }
        self.signers.set(vote_node_id, true);
        self.sig_lists.push(original_signature);

        match vote.get_data() {
            VoteData::ViewSyncPreCommit(_) => {
                *pre_commit_stake_casted += u64::from(vote.get_vote_token().vote_count());
                pre_commit_vote_map.insert(
                    encoded_key,
                    (vote.get_signature(), vote.get_data(), vote.get_vote_token()),
                );
            }
            VoteData::ViewSyncCommit(_) => {
                *commit_stake_casted += u64::from(vote.get_vote_token().vote_count());
                commit_vote_map.insert(
                    encoded_key,
                    (vote.get_signature(), vote.get_data(), vote.get_vote_token()),
                );
            }
            VoteData::ViewSyncFinalize(_) => {
                *finalize_stake_casted += u64::from(vote.get_vote_token().vote_count());
                finalize_vote_map.insert(
                    encoded_key,
                    (vote.get_signature(), vote.get_data(), vote.get_vote_token()),
                );
            }
            _ => unimplemented!(),
        }

        if *pre_commit_stake_casted >= u64::from(self.failure_threshold) {
            let real_qc_pp = <TYPES::SignatureKey as SignatureKey>::get_public_parameter(
                stake_table_entries,
                U256::from(self.failure_threshold.get()),
            );

            let real_qc_sig = <TYPES::SignatureKey as SignatureKey>::assemble(
                &real_qc_pp,
                self.signers.as_bitslice(),
                &self.sig_lists[..],
            );

            self.pre_commit_vote_outcomes
                .remove(&vote_commitment)
                .unwrap();
            return Either::Right(AssembledSignature::ViewSyncPreCommit(real_qc_sig));
        }

        if *commit_stake_casted >= u64::from(self.success_threshold) {
            let real_qc_pp = <TYPES::SignatureKey as SignatureKey>::get_public_parameter(
                stake_table_entries.clone(),
                U256::from(self.success_threshold.get()),
            );

            let real_qc_sig = <TYPES::SignatureKey as SignatureKey>::assemble(
                &real_qc_pp,
                self.signers.as_bitslice(),
                &self.sig_lists[..],
            );
            self.commit_vote_outcomes.remove(&vote_commitment).unwrap();
            return Either::Right(AssembledSignature::ViewSyncCommit(real_qc_sig));
        }

        if *finalize_stake_casted >= u64::from(self.success_threshold) {
            let real_qc_pp = <TYPES::SignatureKey as SignatureKey>::get_public_parameter(
                stake_table_entries.clone(),
                U256::from(self.success_threshold.get()),
            );

            let real_qc_sig = <TYPES::SignatureKey as SignatureKey>::assemble(
                &real_qc_pp,
                self.signers.as_bitslice(),
                &self.sig_lists[..],
            );
            self.finalize_vote_outcomes
                .remove(&vote_commitment)
                .unwrap();
            return Either::Right(AssembledSignature::ViewSyncFinalize(real_qc_sig));
        }

        Either::Left(self)
    }
}

/// Mapping of commitments to vote tokens by key.
type VoteMap<COMMITMENT, TOKEN> = HashMap<
    COMMITMENT,
    (
        u64,
        BTreeMap<EncodedPublicKey, (EncodedSignature, VoteData<COMMITMENT>, TOKEN)>,
    ),
>;
