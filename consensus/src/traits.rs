//! Contains the [`ConsensusApi`] trait.

use async_trait::async_trait;
use commit::Commitment;
use hotshot_types::{
    data::{Leaf, QuorumCertificate, ViewNumber},
    error::HotShotError,
    event::{Event, EventType},
    traits::{
        network::NetworkError,
        node_implementation::{NodeImplementation, TypeMap},
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey}, election::{Election, ElectionError},
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

    /// Retuns the maximum transactions allowed in a block
    fn max_transactions(&self) -> NonZeroUsize;

    /// Returns the minimum transactions that must be in a block
    fn min_transactions(&self) -> usize;

    /// Generates and encodes a vote token
    #[allow(clippy::type_complexity)]
    fn generate_vote_token(
        &self,
        view_number: ViewNumber,
        next_state: Commitment<Leaf<I::StateType>>,
    ) -> Result<Option<<I::Election as Election<I::SignatureKey, ViewNumber>>::VoteTokenType>, ElectionError>;

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
    async fn send_event(&self, event: Event<I::StateType>);

    /// Get a reference to the public key.
    fn public_key(&self) -> &I::SignatureKey;

    /// Get a reference to the private key.
    fn private_key(&self) -> &<I::SignatureKey as SignatureKey>::PrivateKey;

    // Utility functions

    /// returns `true` if the current node is a leader for the given `view_number`
    async fn is_leader(&self, view_number: ViewNumber) -> bool {
        &self.get_leader(view_number).await == self.public_key()
    }

    /// returns `true` if the current node should act as a replica for the given `view_number`
    async fn is_replica(&self, view_number: ViewNumber) -> bool {
        self.leader_acts_as_replica() || !self.is_leader(view_number).await
    }

    /// notifies client of an error
    async fn send_view_error(&self, view_number: ViewNumber, error: Arc<HotShotError>) {
        self.send_event(Event {
            view_number,
            event: EventType::Error { error },
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
    async fn send_decide(&self, view_number: ViewNumber, leaf_views: Vec<Leaf<I::StateType>>) {
        self.send_event(Event {
            view_number,
            event: EventType::Decide {
                leaf_chain: Arc::new(leaf_views),
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
        leaf_commitment: &Commitment<Leaf<I::StateType>>,
        _view_number: ViewNumber,
    ) -> (EncodedPublicKey, EncodedSignature) {
        let signature = I::SignatureKey::sign(self.private_key(), leaf_commitment.as_ref());
        (self.public_key().to_bytes(), signature)
    }

    /// Signs a proposal
    fn sign_proposal(
        &self,
        leaf_commitment: &Commitment<Leaf<I::StateType>>,
        _view_number: ViewNumber,
    ) -> EncodedSignature {
        let signature = I::SignatureKey::sign(self.private_key(), leaf_commitment.as_ref());
        signature
    }

    /// Validate a quorum certificate by checking
    /// signatures
    fn validate_qc(&self, quorum_certificate: &QuorumCertificate<I::StateType>) -> bool;

    /// Check if a signature is valid
    fn is_valid_signature(
        &self,
        encoded_key: &EncodedPublicKey,
        encoded_signature: &EncodedSignature,
        hash: Commitment<Leaf<I::StateType>>,
    ) -> bool;
}
