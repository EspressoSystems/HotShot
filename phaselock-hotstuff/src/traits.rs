//! Contains the [`ConsensusApi`] trait.

use async_trait::async_trait;
use phaselock_types::{
    data::{LeafHash, QuorumCertificate, Stage, VecQuorumCertificate, VerifyHash, ViewNumber},
    event::{Event, EventType},
    message::ConsensusMessage,
    traits::{
        network::NetworkError,
        node_implementation::{NodeImplementation, TypeMap},
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
    },
};
use std::{num::NonZeroUsize, sync::Arc, time::Duration};

/// The API that [`HotStuff`] needs to talk to the system. This should be implemented in the `phaselock` crate and passed to all functions on `HotStuff`.
///
/// [`HotStuff`]: struct.HotStuff.html
#[async_trait]
pub trait ConsensusApi<I: NodeImplementation<N>, const N: usize>: Send + Sync {
    /// Total number of nodes in the network. Also known as `n`.
    fn total_nodes(&self) -> NonZeroUsize;

    /// The amount of nodes that are required to reach a decision. Also known as `n - f`.
    fn threshold(&self) -> NonZeroUsize;

    /// The minimum amount of time a leader has to wait before sending a propose
    fn propose_min_round_time(&self) -> Duration;

    /// The maximum amount of time a leader can wait before sending a propose.
    /// If this time is reached, the leader has to send a propose without transactions.
    fn propose_max_round_time(&self) -> Duration;

    /// Get a reference to the storage implementation
    fn storage(&self) -> &I::Storage;

    /// Returns `true` if the leader should also act as a replica.  This will make the leader cast votes.
    fn leader_acts_as_replica(&self) -> bool;

    /// Returns the `I::SignatureKey` of the leader for the given round and stage
    async fn get_leader(&self, view_number: ViewNumber, stage: Stage) -> I::SignatureKey;

    /// Returns `true` if hotstuff should start the given round. A round can also be started manually by sending `NewView` to the leader.
    ///
    /// In production code this should probably always return `true`.
    async fn should_start_round(&self, view_number: ViewNumber) -> bool;

    /// Send a direct message to the given recipient
    async fn send_direct_message(
        &mut self,
        recipient: I::SignatureKey,
        message: <I as TypeMap<N>>::ConsensusMessage,
    ) -> std::result::Result<(), NetworkError>;

    /// Send a broadcast message to the entire network.
    async fn send_broadcast_message(
        &mut self,
        message: <I as TypeMap<N>>::ConsensusMessage,
    ) -> std::result::Result<(), NetworkError>;

    /// Notify the system of an event within `phaselock-hotstuff`.
    async fn send_event(&mut self, event: Event<I::Block, I::State>);

    /// Get a reference to the public key.
    fn public_key(&self) -> &I::SignatureKey;

    /// Get a reference to the private key.
    fn private_key(&self) -> &<I::SignatureKey as SignatureKey>::PrivateKey;

    /// The `phaselock-hotstuff` implementation will call this method, with the series of blocks and states
    /// that are being committed, whenever a commit action takes place.
    ///
    /// The provided states and blocks are guaranteed to be in ascending order of age (newest to
    /// oldest).
    async fn notify(&self, blocks: Vec<I::Block>, states: Vec<I::State>);

    // Utility functions

    /// returns `true` if the current node is a leader for the given `view_number` and `stage`
    async fn is_leader(&self, view_number: ViewNumber, stage: Stage) -> bool {
        &self.get_leader(view_number, stage).await == self.public_key()
    }

    /// sends a proposal event down the channel
    async fn send_propose(&mut self, view_number: ViewNumber, block: I::Block) {
        self.send_event(Event {
            view_number,
            stage: Stage::Prepare,
            event: EventType::Propose {
                block: Arc::new(block),
            },
        })
        .await;
    }

    /// sends a decide event down the channel
    async fn send_decide(
        &mut self,
        view_number: ViewNumber,
        blocks: Vec<I::Block>,
        states: Vec<I::State>,
        qcs: Vec<VecQuorumCertificate>,
    ) {
        self.send_event(Event {
            view_number,
            stage: Stage::Prepare,
            event: EventType::Decide {
                block: Arc::new(blocks),
                state: Arc::new(states),
                qcs: Arc::new(qcs),
            },
        })
        .await;
    }

    /// Create a [`VerifyHash`] for a given [`LeafHash`], [`Stage`] and [`ViewNumber`]
    fn create_verify_hash(
        &self,
        leaf_hash: &LeafHash<N>,
        stage: Stage,
        view_number: ViewNumber,
    ) -> VerifyHash<32>;

    /// Signs a vote
    fn sign_vote(
        &self,
        leaf_hash: &LeafHash<N>,
        stage: Stage,
        view_number: ViewNumber,
    ) -> (EncodedPublicKey, EncodedSignature) {
        let hash = self.create_verify_hash(leaf_hash, stage, view_number);
        let signature = I::SignatureKey::sign(self.private_key(), hash.as_ref());
        (self.public_key().to_bytes(), signature)
    }

    /// Validate a quorum certificate
    fn validate_qc(
        &self,
        quorum_certificate: &QuorumCertificate<N>,
        view_number: ViewNumber,
        stage: Stage,
    ) -> bool;

    /// Validate this message on if the QC is correct, if it has one
    ///
    /// If this message has no QC then this will return `true`
    fn validate_qc_in_message(&self, message: &ConsensusMessage<I::Block, I::State, N>) -> bool {
        let (qc, view_number, stage) = match message {
            ConsensusMessage::PreCommit(pre_commit) => {
                // PreCommit QC has the votes of the Prepare phase, therefor we must compare against Prepare and not PreCommit
                (&pre_commit.qc, pre_commit.current_view, Stage::Prepare)
            }
            // Same as PreCommit, we compare with 1 stage earlier
            ConsensusMessage::Commit(commit) => (&commit.qc, commit.current_view, Stage::PreCommit),
            ConsensusMessage::Decide(decide) => (&decide.qc, decide.current_view, Stage::Commit),

            ConsensusMessage::NewView(_)
            | ConsensusMessage::Prepare(_)
            | ConsensusMessage::CommitVote(_)
            | ConsensusMessage::PreCommitVote(_)
            | ConsensusMessage::PrepareVote(_) => return true,
        };

        self.validate_qc(qc, view_number, stage)
    }
}
