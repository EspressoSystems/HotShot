//! Provides an event-streaming handle for a [`SystemContext`] running in the background

use crate::{traits::NodeImplementation, types::Event, SystemContext};
use async_broadcast::{Receiver, Sender};

use async_lock::RwLock;
use commit::Committable;
use futures::Stream;

use hotshot_task_impls::events::HotShotEvent;
#[cfg(feature = "hotshot-testing")]
use hotshot_types::{
    message::{MessageKind, SequencingMessage},
    traits::election::Membership,
};

use hotshot_task::task::TaskRegistry;
use hotshot_types::{boxed_sync, simple_vote::QuorumData, BoxSyncFuture};
use hotshot_types::{
    consensus::Consensus,
    data::Leaf,
    error::HotShotError,
    event::EventType,
    simple_certificate::QuorumCertificate,
    traits::{node_implementation::NodeType, state::ConsensusTime, storage::Storage},
};
use std::sync::Arc;
use tracing::error;

/// Event streaming handle for a [`SystemContext`] instance running in the background
///
/// This type provides the means to message and interact with a background [`SystemContext`] instance,
/// allowing the ability to receive [`Event`]s from it, send transactions to it, and interact with
/// the underlying storage.
#[derive(Clone)]
pub struct SystemContextHandle<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// The [sender](ChannelStream) for the output stream from the background process
    pub(crate) output_event_stream: (Sender<Event<TYPES>>, Receiver<Event<TYPES>>),
    /// access to the internal ev ent stream, in case we need to, say, shut something down
    pub(crate) internal_event_stream: (Sender<HotShotEvent<TYPES>>, Receiver<HotShotEvent<TYPES>>),
    /// registry for controlling tasks
    pub(crate) registry: Arc<TaskRegistry>,

    /// Internal reference to the underlying [`SystemContext`]
    pub hotshot: SystemContext<TYPES, I>,

    /// Our copy of the `Storage` view for a hotshot
    pub(crate) storage: I::Storage,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES> + 'static> SystemContextHandle<TYPES, I> {
    /// obtains a stream to expose to the user
    pub async fn get_event_stream(&self) -> impl Stream<Item = Event<TYPES>> {
        self.output_event_stream.1.clone()
    }

    /// HACK so we can know the types when running tests...
    /// there are two cleaner solutions:
    /// - make the stream generic and in nodetypes or nodeimpelmentation
    /// - type wrapper
    pub async fn get_event_stream_known_impl(&self) -> Receiver<Event<TYPES>> {
        self.output_event_stream.1.clone()
    }

    /// HACK so we can know the types when running tests...
    /// there are two cleaner solutions:
    /// - make the stream generic and in nodetypes or nodeimpelmentation
    /// - type wrapper
    /// NOTE: this is only used for sanity checks in our tests
    pub async fn get_internal_event_stream_known_impl(&self) -> Receiver<HotShotEvent<TYPES>> {
        self.internal_event_stream.1.clone()
    }

    /// Gets the current committed state of the [`SystemContext`] instance
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying `Storage` returns an error
    pub async fn get_state(&self) {
        self.hotshot.get_state().await;
    }

    /// Gets most recent decided leaf
    /// # Panics
    ///
    /// Panics if internal consensus is in an inconsistent state.
    pub async fn get_decided_leaf(&self) -> Leaf<TYPES> {
        self.hotshot.get_decided_leaf().await
    }

    /// Submits a transaction to the backing [`SystemContext`] instance.
    ///
    /// The current node broadcasts the transaction to all nodes on the network.
    ///
    /// # Errors
    ///
    /// Will return a [`HotShotError`] if some error occurs in the underlying
    /// [`SystemContext`] instance.
    pub async fn submit_transaction(
        &self,
        tx: TYPES::Transaction,
    ) -> Result<(), HotShotError<TYPES>> {
        self.hotshot.publish_transaction_async(tx).await
    }

    /// performs the genesis initializaiton
    pub async fn maybe_do_genesis_init(&self) {
        let _anchor = self.storage();
        if let Ok(anchor_leaf) = self.storage().get_anchored_view().await {
            if anchor_leaf.view_number == TYPES::Time::genesis() {
                let leaf = Leaf::from_stored_view(anchor_leaf);
                let mut qc = QuorumCertificate::<TYPES>::genesis();
                qc.data = QuorumData {
                    leaf_commit: leaf.commit(),
                };
                let event = Event {
                    view_number: TYPES::Time::genesis(),
                    event: EventType::Decide {
                        leaf_chain: Arc::new(vec![leaf]),
                        qc: Arc::new(qc),
                        block_size: None,
                    },
                };
                let _ = self.output_event_stream.0.broadcast_direct(event).await;
            }
        } else {
            // TODO (justin) this seems bad. I think we should hard error in this case??
            error!("Hotshot storage has no anchor leaf!");
        }
    }

    /// begin consensus by sending a genesis event
    /// Use `start_consensus` on `SystemContext` instead
    #[deprecated]
    pub async fn start_consensus_deprecated(&self) {
        self.maybe_do_genesis_init().await;
    }

    /// Provides a reference to the underlying storage for this [`SystemContext`], allowing access to
    /// historical data
    pub fn storage(&self) -> &I::Storage {
        &self.storage
    }

    /// Get the underlying consensus state for this [`SystemContext`]
    pub fn get_consensus(&self) -> Arc<RwLock<Consensus<TYPES>>> {
        self.hotshot.get_consensus()
    }

    /// Block the underlying quorum (and committee) networking interfaces until node is
    /// successfully initialized into the networks.
    pub async fn wait_for_networks_ready(&self) {
        self.hotshot.inner.networks.wait_for_networks_ready().await;
    }

    /// Shut down the the inner hotshot and wait until all background threads are closed.
    //     pub async fn shut_down(mut self) {
    //         self.registry.shutdown_all().await
    pub fn shut_down<'a, 'b>(&'a mut self) -> BoxSyncFuture<'b, ()>
    where
        'a: 'b,
        Self: 'b,
    {
        boxed_sync(async move {
            self.hotshot.inner.networks.shut_down_networks().await;
            self.registry.shutdown().await;
        })
    }

    /// return the timeout for a view of the underlying `SystemContext`
    pub fn get_next_view_timeout(&self) -> u64 {
        self.hotshot.get_next_view_timeout()
    }

    // Below is for testing only:

    /// Wrapper for `HotShotConsensusApi`'s `get_leader` function
    #[allow(clippy::unused_async)] // async for API compatibility reasons
    #[cfg(feature = "hotshot-testing")]
    pub async fn get_leader(&self, view_number: TYPES::Time) -> TYPES::SignatureKey {
        self.hotshot
            .inner
            .memberships
            .quorum_membership
            .get_leader(view_number)
    }

    /// Wrapper to get this node's public key
    #[cfg(feature = "hotshot-testing")]
    pub fn get_public_key(&self) -> TYPES::SignatureKey {
        self.hotshot.inner.public_key.clone()
    }

    /// Wrapper to get this node's current view
    #[cfg(feature = "hotshot-testing")]
    pub async fn get_current_view(&self) -> TYPES::Time {
        self.hotshot.inner.consensus.read().await.cur_view
    }

    /// Wrapper around `HotShotConsensusApi`'s `send_broadcast_consensus_message` function
    #[cfg(feature = "hotshot-testing")]
    pub async fn send_broadcast_consensus_message(&self, msg: SequencingMessage<TYPES>) {
        let _result = self
            .hotshot
            .send_broadcast_message(MessageKind::from_consensus_message(msg))
            .await;
    }

    /// Wrapper around `HotShotConsensusApi`'s `send_direct_consensus_message` function
    #[cfg(feature = "hotshot-testing")]
    pub async fn send_direct_consensus_message(
        &self,
        msg: SequencingMessage<TYPES>,
        recipient: TYPES::SignatureKey,
    ) {
        let _result = self
            .hotshot
            .send_direct_message(MessageKind::from_consensus_message(msg), recipient)
            .await;
    }
}
