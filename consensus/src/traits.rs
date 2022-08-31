//! Contains the [`ConsensusApi`] trait.

use async_trait::async_trait;
use commit::Commitment;
use hotshot_types::{
    data::{Leaf, QuorumCertificate, ViewNumber},
    event::{Event, EventType, TransactionCommitment},
    traits::{
        network::NetworkError,
        node_implementation::{NodeImplementation, TypeMap},
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
        StateContents,
    },
};
use std::{num::NonZeroUsize, sync::Arc, time::Duration};

/// The API that [`HotStuff`] needs to talk to the system. This should be implemented in the `hotshot` crate and passed to all functions on `HotStuff`.
///
/// [`HotStuff`]: struct.HotStuff.html
#[async_trait]
pub trait ConsensusApi<I: NodeImplementation>: Send + Sync {
    /// Total number of nodes in the network. Also known as `n`.
    fn total_nodes(&self) -> NonZeroUsize;

    /// The amount of nodes that are required to reach a decision. Also known as `n - f`.
    fn threshold(&self) -> NonZeroUsize;

    /// The minimum amount of time a leader has to wait before sending a propose
    fn propose_min_round_time(&self) -> Duration;

    /// The maximum amount of time a leader can wait before sending a propose.ConsensusApi
    /// If this time is reached, the leader has to send a propose without transactions.
    fn propose_max_round_time(&self) -> Duration;

    /// Get a reference to the storage implementation
    fn storage(&self) -> &I::Storage;

    /// Returns `true` if the leader should also act as a replica.  This will make the leader cast votes.
    fn leader_acts_as_replica(&self) -> bool;

    /// Returns the `I::SignatureKey` of the leader for the given round and stage
    async fn get_leader(&self, view_number: ViewNumber) -> I::SignatureKey;

    /// Returns `true` if hotstuff should start the given round. A round can also be started manually by sending `NewView` to the leader.
    ///
    /// In production code this should probably always return `true`.
    async fn should_start_round(&self, view_number: ViewNumber) -> bool;

    /// Send a direct message to the given recipient
    async fn send_direct_message(
        &self,
        recipient: I::SignatureKey,
        message: <I as TypeMap>::ConsensusMessage,
    ) -> std::result::Result<(), NetworkError>;

    /// Send a broadcast message to the entire network.
    async fn send_broadcast_message(
        &self,
        message: <I as TypeMap>::ConsensusMessage,
    ) -> std::result::Result<(), NetworkError>;

    /// Notify the system of an event within `hotshot-consensus`.
    async fn send_event(&self, event: Event<I::State>);

    /// Get a reference to the public key.
    fn public_key(&self) -> &I::SignatureKey;

    /// Get a reference to the private key.
    fn private_key(&self) -> &<I::SignatureKey as SignatureKey>::PrivateKey;

    /// The `hotshot-consensus` implementation will call this method, with the series of blocks and states
    /// that are being committed, whenever a commit action takes place.
    ///
    /// The provided states and blocks are guaranteed to be in ascending order of age (newest to
    /// oldest).
    async fn notify(&self, blocks: Vec<<I::State as StateContents>::Block>, states: Vec<I::State>);

    // Utility functions

    /// returns `true` if the current node is a leader for the given `view_number`
    async fn is_leader(&self, view_number: ViewNumber) -> bool {
        &self.get_leader(view_number).await == self.public_key()
    }

    /// returns `true` if the current node should act as a replica for the given `view_number`
    async fn is_replica(&self, view_number: ViewNumber) -> bool {
        self.leader_acts_as_replica() || !self.is_leader(view_number).await
    }

    /// sends a proposal event down the channel
    async fn send_propose(
        &self,
        view_number: ViewNumber,
        block: <I::State as StateContents>::Block,
    ) {
        self.send_event(Event {
            view_number,
            event: EventType::Propose {
                block: Arc::new(block),
            },
        })
        .await;
    }

    /// notifies client of a replica timeout
    async fn send_replica_timeout(&self, view_number: ViewNumber) {
        self.send_event(Event {
            view_number,
            event: EventType::ReplicaViewTimeout { view_number },
        })
        .await;
    }

    /// notifies client of a next leader timeout
    async fn send_next_leader_timeout(&self, view_number: ViewNumber) {
        self.send_event(Event {
            view_number,
            event: EventType::NextLeaderViewTimeout { view_number },
        })
        .await;
    }

    /// sends a decide event down the channel
    async fn send_decide(
        &self,
        view_number: ViewNumber,
        blocks: Vec<<I::State as StateContents>::Block>,
        states: Vec<I::State>,
        qcs: Vec<QuorumCertificate<I::State>>,
        rejects: Vec<Vec<TransactionCommitment<I::State>>>,
    ) {
        self.send_event(Event {
            view_number,
            event: EventType::Decide {
                block: Arc::new(blocks),
                state: Arc::new(states),
                qcs: Arc::new(qcs),
                rejects: Arc::new(rejects),
            },
        })
        .await;
    }

    /// Sends a `ViewFinished` event
    async fn send_view_finished(&self, view_number: ViewNumber) {
        self.send_event(Event {
            view_number,
            event: EventType::ViewFinished { view_number },
        })
        .await;
    }

    /// Signs a vote
    fn sign_vote(
        &self,
        leaf_commitment: &Commitment<Leaf<I::State>>,
        _view_number: ViewNumber,
    ) -> (EncodedPublicKey, EncodedSignature) {
        let signature = I::SignatureKey::sign(self.private_key(), leaf_commitment.as_ref());
        (self.public_key().to_bytes(), signature)
    }

    /// Signs a proposal
    fn sign_proposal(
        &self,
        leaf_commitment: &Commitment<Leaf<I::State>>,
        _view_number: ViewNumber,
    ) -> EncodedSignature {
        let signature = I::SignatureKey::sign(self.private_key(), leaf_commitment.as_ref());
        signature
    }

    /// Validate a quorum certificate by checking
    /// signatures
    fn validate_qc(&self, quorum_certificate: &QuorumCertificate<I::State>) -> bool;

    /// Check if a signature is valid
    fn is_valid_signature(
        &self,
        encoded_key: &EncodedPublicKey,
        encoded_signature: &EncodedSignature,
        hash: Commitment<Leaf<I::State>>,
    ) -> bool;
}
