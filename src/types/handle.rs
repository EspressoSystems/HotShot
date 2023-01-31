//! Provides an event-streaming handle for a [`HotShot`] running in the background

use crate::{
    traits::{NetworkError::ShutDown, NodeImplementation},
    types::{Event, HotShotError::NetworkFault},
    HotShot,
};
use async_compatibility_layer::async_primitives::broadcast::{BroadcastReceiver, BroadcastSender};
use commit::Committable;
use hotshot_types::{
    data::LeafType,
    error::{HotShotError, RoundTimedoutState},
    event::EventType,
    traits::{
        election::SignedCertificate, network::NetworkingImplementation,
        node_implementation::NodeType, state::ConsensusTime, storage::Storage,
    },
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tracing::{debug, error};

#[cfg(feature = "hotshot-testing")]
use crate::HotShotConsensusApi;
#[cfg(feature = "hotshot-testing")]
use commit::Commitment;
#[cfg(feature = "hotshot-testing")]
use hotshot_consensus::ConsensusApi;
#[cfg(feature = "hotshot-testing")]
use hotshot_types::{
    message::ConsensusMessage,
    traits::signature_key::{EncodedPublicKey, EncodedSignature},
};

/// Event streaming handle for a [`HotShot`] instance running in the background
///
/// This type provides the means to message and interact with a background [`HotShot`] instance,
/// allowing the ability to receive [`Event`]s from it, send transactions to it, and interact with
/// the underlying storage.
pub struct HotShotHandle<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// The [sender](BroadcastSender) for the output stream from the background process
    ///
    /// This is kept around as an implementation detail, as the [`BroadcastSender::handle_async`]
    /// method is needed to generate new receivers for cloning the handle.
    pub(crate) sender_handle: Arc<BroadcastSender<Event<TYPES, I::Leaf>>>,
    /// Internal reference to the underlying [`HotShot`]
    pub(crate) hotshot: HotShot<TYPES::ConsensusType, TYPES, I>,
    /// The [`BroadcastReceiver`] we get the events from
    pub(crate) stream_output: BroadcastReceiver<Event<TYPES, I::Leaf>>,
    /// Global to signify the `HotShot` should be closed after completing the next round
    pub(crate) shut_down: Arc<AtomicBool>,
    /// Our copy of the `Storage` view for a hotshot
    pub(crate) storage: I::Storage,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES> + 'static> Clone for HotShotHandle<TYPES, I> {
    fn clone(&self) -> Self {
        Self {
            sender_handle: self.sender_handle.clone(),
            stream_output: self.sender_handle.handle_sync(),
            hotshot: self.hotshot.clone(),
            shut_down: self.shut_down.clone(),
            storage: self.storage.clone(),
        }
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES> + 'static> HotShotHandle<TYPES, I> {
    /// Will return the next event in the queue
    ///
    /// # Errors
    ///
    /// Will return [`HotShotError::NetworkFault`] if the underlying [`HotShot`] has been closed.
    pub async fn next_event(&mut self) -> Result<Event<TYPES, I::Leaf>, HotShotError<TYPES>> {
        let result = self.stream_output.recv_async().await;
        match result {
            Ok(result) => Ok(result),
            Err(_) => Err(NetworkFault { source: ShutDown }),
        }
    }
    /// Will attempt to immediately pull an event out of the queue
    ///
    /// # Errors
    ///
    /// Will return [`HotShotError::NetworkFault`] if the underlying [`HotShot`] instance has shut down
    pub fn try_next_event(&mut self) -> Result<Option<Event<TYPES, I::Leaf>>, HotShotError<TYPES>> {
        let result = self.stream_output.try_recv();
        Ok(result)
    }

    /// Will pull all the currently available events out of the event queue.
    ///
    /// # Errors
    ///
    /// Will return [`HotShotError::NetworkFault`] if the underlying [`HotShot`] instance has been shut
    /// down.
    pub fn available_events(&mut self) -> Result<Vec<Event<TYPES, I::Leaf>>, HotShotError<TYPES>> {
        let mut output = vec![];
        // Loop to pull out all the outputs
        loop {
            match self.try_next_event() {
                Ok(Some(x)) => output.push(x),
                Ok(None) => break,
                // // try_next event can only return HotShotError { source: NetworkError::ShutDown }
                Err(x) => return Err(x),
            }
        }
        Ok(output)
    }

    /// Gets the current committed state of the [`HotShot`] instance
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying `Storage` returns an error
    pub async fn get_state(&self) -> <I::Leaf as LeafType>::StateCommitmentType {
        self.hotshot.get_state().await
    }

    /// Gets most recent decided leaf
    /// # Panics
    ///
    /// Panics if internal consensus is in an inconsistent state.
    pub async fn get_decided_leaf(&self) -> I::Leaf {
        self.hotshot.get_decided_leaf().await
    }

    /// Submits a transaction to the backing [`HotShot`] instance.
    ///
    /// The current node broadcasts the transaction to all nodes on the network.
    ///
    /// # Errors
    ///
    /// Will return a [`HotShotError`] if some error occurs in the underlying
    /// [`HotShot`] instance.
    pub async fn submit_transaction(
        &self,
        tx: TYPES::Transaction,
    ) -> Result<(), HotShotError<TYPES>> {
        self.hotshot.publish_transaction_async(tx).await
    }

    /// Signals to the underlying [`HotShot`] to unpause
    ///
    /// This will cause the background task to start running consensus again.
    pub async fn start(&self) {
        self.hotshot.inner.background_task_handle.start().await;
        // if is genesis
        let _anchor = self.storage();
        if let Ok(anchor_leaf) = self.storage().get_anchored_view().await {
            if anchor_leaf.view_number == TYPES::Time::genesis() {
                let leaf: I::Leaf = I::Leaf::from_stored_view(anchor_leaf);
                let mut qc = <I::Leaf as LeafType>::QuorumCertificate::genesis();
                qc.set_leaf_commitment(leaf.commit());
                let event = Event {
                    view_number: TYPES::Time::genesis(),
                    event: EventType::Decide {
                        leaf_chain: Arc::new(vec![leaf]),
                        qc: Arc::new(qc),
                    },
                };
                if self.sender_handle.send_async(event).await.is_err() {
                    error!("Error sending genesis storage event upstream!");
                }
            }
        } else {
            error!("Hotshot storage has no anchor leaf!");
        }
    }

    /// Signals the underlying [`HotShot`] to run one round, if paused.
    ///
    /// Do not call this function if [`HotShot`] has been unpaused by [`HotShotHandle::start`].
    pub async fn start_one_round(&self) {
        self.hotshot
            .inner
            .background_task_handle
            .start_one_round()
            .await;
    }

    /// iterate through all events on a [`NodeImplementation`] and determine if the node finished
    /// successfully
    /// # Errors
    /// Errors if unable to obtain storage
    /// # Panics
    /// Panics if the event stream is shut down while this is running
    pub async fn collect_round_events(
        &mut self,
    ) -> Result<
        (
            Vec<<I::Leaf as LeafType>::StateCommitmentType>,
            Vec<<I::Leaf as LeafType>::DeltasType>,
        ),
        HotShotError<TYPES>,
    > {
        // TODO we should probably do a view check
        // but we can do that later. It's non-obvious how to get the view number out
        // to check against

        // drain all events from this node
        let mut results = Ok((Vec::new(), Vec::new()));
        loop {
            // unwrap is fine here since the thing hasn't been shut down
            let event = self.next_event().await.unwrap();
            match event.event {
                EventType::ReplicaViewTimeout { view_number: time } => {
                    error!(?event, "Replica timed out!");
                    results = Err(HotShotError::ViewTimeoutError {
                        view_number: time,
                        state: RoundTimedoutState::TestCollectRoundEventsTimedOut,
                    });
                }
                EventType::Decide { leaf_chain, .. } => {
                    results = Ok(leaf_chain
                        .iter()
                        .cloned()
                        .map(|leaf| (leaf.get_state(), leaf.get_deltas()))
                        .unzip());
                }
                EventType::ViewFinished { view_number: _ } => return results,
                event => {
                    debug!("recv-ed event {:?}", event);
                }
            }
        }
    }

    /// Provides a reference to the underlying storage for this [`HotShot`], allowing access to
    /// historical data
    pub fn storage(&self) -> &I::Storage {
        &self.storage
    }

    /// Provides a reference to the underlying networking interface for this [`HotShot`], allowing access to
    /// networking stats.
    pub fn networking(&self) -> &I::Networking {
        &self.hotshot.inner.networking
    }

    /// Shut down the the inner hotshot and wait until all background threads are closed.
    pub async fn shut_down(self) {
        self.shut_down.store(true, Ordering::Relaxed);
        self.hotshot.inner.networking.shut_down().await;
        self.hotshot
            .inner
            .background_task_handle
            .wait_shutdown(self.hotshot.send_network_lookup)
            .await;
    }

    /// return the timeout for a view of the underlying `HotShot`
    pub fn get_next_view_timeout(&self) -> u64 {
        self.hotshot.get_next_view_timeout()
    }

    // Below is for testing only:

    /// Wrapper for `HotShotConsensusApi`'s `get_leader` function
    #[cfg(feature = "hotshot-testing")]
    pub async fn get_leader(&self, view_number: TYPES::Time) -> TYPES::SignatureKey {
        let api = HotShotConsensusApi {
            inner: self.hotshot.inner.clone(),
        };
        api.get_leader(view_number).await
    }

    /// Wrapper to get this node's public key
    #[cfg(feature = "hotshot-testing")]
    pub fn get_public_key(&self) -> TYPES::SignatureKey {
        self.hotshot.inner.public_key.clone()
    }

    /// Wrapper to get this node's current view
    #[cfg(feature = "hotshot-testing")]
    pub async fn get_current_view(&self) -> TYPES::Time {
        self.hotshot.hotstuff.read().await.cur_view
    }

    /// Wrapper around `HotShotConsensusApi`'s `sign_validating_or_commitment_proposal` function
    #[cfg(feature = "hotshot-testing")]
    pub fn sign_validating_or_commitment_proposal(
        &self,
        leaf_commitment: &Commitment<I::Leaf>,
    ) -> EncodedSignature {
        let api = HotShotConsensusApi {
            inner: self.hotshot.inner.clone(),
        };
        api.sign_validating_or_commitment_proposal(leaf_commitment)
    }

    /// Wrapper around `HotShotConsensusApi`'s `sign_da_proposal` function
    #[cfg(feature = "hotshot-testing")]
    pub fn sign_da_proposal(
        &self,
        block_commitment: &Commitment<TYPES::BlockType>,
    ) -> EncodedSignature {
        let api = HotShotConsensusApi {
            inner: self.hotshot.inner.clone(),
        };
        api.sign_da_proposal(block_commitment)
    }

    /// Wrapper around `HotShotConsensusApi`'s `sign_da_vote` function
    #[cfg(feature = "hotshot-testing")]
    pub fn sign_da_vote(
        &self,
        block_commitment: Commitment<TYPES::BlockType>,
    ) -> (EncodedPublicKey, EncodedSignature) {
        let api = HotShotConsensusApi {
            inner: self.hotshot.inner.clone(),
        };
        api.sign_da_vote(block_commitment)
    }

    /// Wrapper around `HotShotConsensusApi`'s `sign_yes_vote` function
    #[cfg(feature = "hotshot-testing")]
    pub fn sign_yes_vote(
        &self,
        leaf_commitment: Commitment<I::Leaf>,
    ) -> (EncodedPublicKey, EncodedSignature) {
        let api = HotShotConsensusApi {
            inner: self.hotshot.inner.clone(),
        };
        api.sign_yes_vote(leaf_commitment)
    }

    /// Wrapper around `HotShotConsensusApi`'s `sign_no_vote` function
    #[cfg(feature = "hotshot-testing")]
    pub fn sign_no_vote(
        &self,
        leaf_commitment: Commitment<I::Leaf>,
    ) -> (EncodedPublicKey, EncodedSignature) {
        let api = HotShotConsensusApi {
            inner: self.hotshot.inner.clone(),
        };
        api.sign_no_vote(leaf_commitment)
    }

    /// Wrapper around `HotShotConsensusApi`'s `sign_timeout_vote` function
    #[cfg(feature = "hotshot-testing")]
    pub fn sign_timeout_vote(
        &self,
        view_number: TYPES::Time,
    ) -> (EncodedPublicKey, EncodedSignature) {
        let api = HotShotConsensusApi {
            inner: self.hotshot.inner.clone(),
        };
        api.sign_timeout_vote(view_number)
    }

    /// Wrapper around `HotShotConsensusApi`'s `create_da_message` function
    pub fn create_da_message(
        &self,
        justify_qc_commitment: Commitment<<I::Leaf as LeafType>::QuorumCertificate>,
        block_commitment: Commitment<TYPES::BlockType>,
        current_view: TYPES::Time,
        vote_token: TYPES::VoteTokenType,
    ) -> ConsensusMessage<TYPES, I::Leaf, I::Proposal> {
        let api = HotShotConsensusApi {
            inner: self.hotshot.inner.clone(),
        };
        api.create_da_message(
            justify_qc_commitment,
            block_commitment,
            current_view,
            vote_token,
        )
    }

    /// Wrapper around `HotShotConsensusApi`'s `create_yes_message` function
    pub fn create_yes_message(
        &self,
        justify_qc_commitment: Commitment<<I::Leaf as LeafType>::QuorumCertificate>,
        leaf_commitment: Commitment<I::Leaf>,
        current_view: TYPES::Time,
        vote_token: TYPES::VoteTokenType,
    ) -> ConsensusMessage<TYPES, I::Leaf, I::Proposal> {
        let api = HotShotConsensusApi {
            inner: self.hotshot.inner.clone(),
        };
        api.create_yes_message(
            justify_qc_commitment,
            leaf_commitment,
            current_view,
            vote_token,
        )
    }

    /// Wrapper around `HotShotConsensusApi`'s `create_no_message` function
    pub fn create_no_message(
        &self,
        justify_qc_commitment: Commitment<<I::Leaf as LeafType>::QuorumCertificate>,
        leaf_commitment: Commitment<I::Leaf>,
        current_view: TYPES::Time,
        vote_token: TYPES::VoteTokenType,
    ) -> ConsensusMessage<TYPES, I::Leaf, I::Proposal> {
        let api = HotShotConsensusApi {
            inner: self.hotshot.inner.clone(),
        };
        api.create_no_message(
            justify_qc_commitment,
            leaf_commitment,
            current_view,
            vote_token,
        )
    }

    /// Wrapper around `HotShotConsensusApi`'s `create_timeout_message` function
    pub fn create_timeout_message(
        &self,
        justify_qc: <I::Leaf as LeafType>::QuorumCertificate,
        current_view: TYPES::Time,
        vote_token: TYPES::VoteTokenType,
    ) -> ConsensusMessage<TYPES, I::Leaf, I::Proposal> {
        let api = HotShotConsensusApi {
            inner: self.hotshot.inner.clone(),
        };
        api.create_timeout_message(justify_qc, current_view, vote_token)
    }

    /// Wrapper around `HotShotConsensusApi`'s `send_broadcast_consensus_message` function
    #[cfg(feature = "hotshot-testing")]
    pub async fn send_broadcast_consensus_message(
        &self,
        msg: ConsensusMessage<TYPES, I::Leaf, I::Proposal>,
    ) {
        let _result = self.hotshot.send_broadcast_message(msg).await;
    }

    /// Wrapper around `HotShotConsensusApi`'s `send_direct_consensus_message` function
    #[cfg(feature = "hotshot-testing")]
    pub async fn send_direct_consensus_message(
        &self,
        msg: ConsensusMessage<TYPES, I::Leaf, I::Proposal>,
        recipient: TYPES::SignatureKey,
    ) {
        let _result = self.hotshot.send_direct_message(msg, recipient).await;
    }

    /// Get length of the replica's receiver channel
    #[cfg(feature = "hotshot-testing")]
    pub async fn get_replica_receiver_channel_len(
        &self,
        view_number: TYPES::Time,
    ) -> Option<usize> {
        use async_compatibility_layer::channel::UnboundedReceiver;

        let channel_map = self.hotshot.replica_channel_map.read().await;
        let chan = channel_map.channel_map.get(&view_number)?;
        let receiver = chan.receiver_chan.lock().await;
        UnboundedReceiver::len(&*receiver)
    }

    /// Get length of the next leaders's receiver channel
    #[cfg(feature = "hotshot-testing")]
    pub async fn get_next_leader_receiver_channel_len(
        &self,
        view_number: TYPES::Time,
    ) -> Option<usize> {
        use async_compatibility_layer::channel::UnboundedReceiver;

        let channel_map = self.hotshot.next_leader_channel_map.read().await;
        let chan = channel_map.channel_map.get(&view_number)?;

        let receiver = chan.receiver_chan.lock().await;
        UnboundedReceiver::len(&*receiver)
    }
}
