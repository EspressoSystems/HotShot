//! Contains the [`ConsensusApi`] trait.

use async_trait::async_trait;
use phaselock_types::{
    data::{LeafHash, Stage, ViewNumber},
    event::{Event, EventType},
    message::Vote,
    traits::{
        network::NetworkError,
        node_implementation::{NodeImplementation, TypeMap},
        signature_key::SignatureKey,
    },
    PubKey,
};

use std::{
    collections::HashSet,
    num::{NonZeroU64, NonZeroUsize},
    sync::Arc,
    time::Duration,
};

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

    /// Returns the `PubKey` of the leader for the given round and stage
    async fn get_leader(&self, view_number: ViewNumber, stage: Stage) -> PubKey<I::SigningKey>;

    /// Returns `true` if hotstuff should start the given round. A round can also be started manually by sending `NewView` to the leader.
    ///
    /// In production code this should probably always return `true`.
    async fn should_start_round(&self, view_number: ViewNumber) -> bool;

    /// Send a direct message to the given recipient
    async fn send_direct_message(
        &mut self,
        recipient: PubKey<I::SigningKey>,
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
    fn public_key(&self) -> &PubKey<I::SigningKey>;

    /// Get a reference to the private key.
    fn private_key(&self) -> &<I::SigningKey as SignatureKey>::PrivateKey;

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
    ) {
        self.send_event(Event {
            view_number,
            stage: Stage::Prepare,
            event: EventType::Decide {
                block: Arc::new(blocks),
                state: Arc::new(states),
            },
        })
        .await;
    }

    /// Combines the signatures
    fn combine_signatures(&self, votes: &[Vote<N>]) -> Option<Vec<Vec<u8>>>;

    /// Validate a QC for the current round
    ///
    /// TODO(#170): Integrate this with the election trait to provide support
    /// for committe election
    fn cluster_public_keys(&self) -> &HashSet<I::SigningKey>;

    /// Signs a vote using the node's signing key
    ///
    /// TODO(#170): Push this functionality into the election trait
    fn sign_vote(&self, leaf_hash: &LeafHash<N>, stage: Stage, view_number: u64) -> Vec<u8>;
}
