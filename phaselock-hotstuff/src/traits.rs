use async_trait::async_trait;
use phaselock_types::{
    data::Stage,
    event::{Event, EventType},
    traits::{
        network::NetworkError,
        node_implementation::{NodeImplementation, TypeMap},
    },
    PrivKey, PubKey,
};
use std::{num::NonZeroU64, sync::Arc, time::Duration};

#[async_trait]
pub trait ConsensusApi<I: NodeImplementation<N>, const N: usize>: Send + Sync {
    /// Total number of nodes in the network. Also known as `n`.
    fn total_nodes(&self) -> u64;

    /// The amount of nodes that are required to reach a decision. Also known as `n - f`.
    fn threshold(&self) -> NonZeroU64;

    /// The minimum amount of time a leader has to wait before sending a propose
    fn propose_min_round_time(&self) -> Duration;

    /// The maximum amount of time a leader can wait before sending a propose.
    /// If this time is reached, the leader has to send a propose without transactions.
    fn propose_max_round_time(&self) -> Duration;

    /// Get a reference to the storage implementation
    fn storage(&self) -> &I::Storage;

    /// Returns `true` if the leader should also act as a replica.  This will make the leader cast votes.
    fn leader_acts_as_replica(&self) -> bool;

    /// Returns the `PubKey` of the leader for the given round
    async fn get_leader_for_round(&self, view_number: u64) -> PubKey;

    async fn send_direct_message(
        &self,
        recipient: PubKey,
        message: <I as TypeMap<N>>::ConsensusMessage,
    ) -> std::result::Result<(), NetworkError>;

    async fn send_broadcast_message(
        &self,
        message: <I as TypeMap<N>>::ConsensusMessage,
    ) -> std::result::Result<(), NetworkError>;

    /// Notify the system of an event within `phaselock-hotstuff`.
    async fn send_event(&self, event: Event<I::Block, I::State>);

    /// Get a reference to the public key.
    fn public_key(&self) -> &PubKey;

    /// Get a reference to the private key.
    fn private_key(&self) -> &PrivKey;

    /// The `phaselock-hotstuff` implementation will call this method, with the series of blocks and states
    /// that are being committed, whenever a commit action takes place.
    ///
    /// The provided states and blocks are guaranteed to be in ascending order of age (newest to
    /// oldest).
    async fn notify(&self, blocks: Vec<I::Block>, states: Vec<I::State>);

    // Utility functions

    /// Returns `true` if this node is leader this round.
    async fn is_leader_this_round(&self, view_number: u64) -> bool {
        &self.get_leader_for_round(view_number).await == self.public_key()
    }

    /// sends a proposal event down the channel
    async fn send_propose(&self, view_number: u64, block: &I::Block) {
        self.send_event(Event {
            view_number,
            stage: Stage::Prepare,
            event: EventType::Propose {
                block: Arc::new(block.clone()),
            },
        })
        .await;
    }

    /// sends a decide event down the channel
    async fn send_decide(&self, view_number: u64, blocks: &[(I::Block, I::State)]) {
        self.send_event(Event {
            view_number,
            stage: Stage::Prepare,
            event: EventType::Decide {
                block: Arc::new(blocks.iter().map(|x| x.0.clone()).collect()),
                state: Arc::new(blocks.iter().map(|x| x.1.clone()).collect()),
            },
        })
        .await;
    }
}
