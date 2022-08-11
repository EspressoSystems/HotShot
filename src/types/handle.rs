//! Provides an event-streaming handle for a [`HotShot`] running in the background

use crate::{
    traits::{BlockContents, NetworkError::ShutDown, NodeImplementation},
    types::{Event, HotShotError::NetworkFault},
    HotShot, Result,
};
use async_std::{task::block_on, prelude::FutureExt};
use hotshot_types::{
    traits::network::NetworkingImplementation, error::{HotShotError, TimeoutSnafu, RoundTimedoutState}, event::EventType,
};
use hotshot_utils::{
    broadcast::{BroadcastReceiver, BroadcastSender},
    hack::nll_todo,
};
use snafu::ResultExt;
use tracing::{error, debug};

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    }, time::Duration,
};


/// Event streaming handle for a [`HotShot`] instance running in the background
///
/// This type provides the means to message and interact with a background [`HotShot`] instance,
/// allowing the ability to receive [`Event`]s from it, send transactions to it, and interact with
/// the underlying storage.
pub struct HotShotHandle<I: NodeImplementation<N> + Send + Sync + 'static, const N: usize> {
    /// The [sender](BroadcastSender) for the output stream from the background process
    ///
    /// This is kept around as an implementation detail, as the [`BroadcastSender::handle_async`]
    /// method is needed to generate new receivers for cloning the handle.
    pub(crate) sender_handle: Arc<BroadcastSender<Event<I::Block, I::State, N>>>,
    /// Internal reference to the underlying [`HotShot`]
    pub(crate) hotshot: HotShot<I, N>,
    /// The [`BroadcastReceiver`] we get the events from
    pub(crate) stream_output: BroadcastReceiver<Event<I::Block, I::State, N>>,
    /// Global to signify the `HotShot` should be closed after completing the next round
    pub(crate) shut_down: Arc<AtomicBool>,
    /// Our copy of the `Storage` view for a hotshot
    pub(crate) storage: I::Storage,
}

impl<B: NodeImplementation<N> + 'static, const N: usize> Clone for HotShotHandle<B, N> {
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

impl<I: NodeImplementation<N> + 'static, const N: usize> HotShotHandle<I, N> {
    /// Will return the next event in the queue
    ///
    /// # Errors
    ///
    /// Will return [`HotShotError::NetworkFault`] if the underlying [`HotShot`] has been closed.
    pub async fn next_event(&mut self) -> Result<Event<I::Block, I::State, N>> {
        let result = self.stream_output.recv_async().await;
        match result {
            Ok(result) => Ok(result),
            Err(_) => Err(NetworkFault { source: ShutDown }),
        }
    }
    /// Synchronous version of `next_event`
    ///
    /// Will internally call `block_on` on `next_event`
    ///
    /// # Errors
    ///
    /// See documentation for `next_event`
    pub fn next_event_sync(&mut self) -> Result<Event<I::Block, I::State, N>> {
        block_on(self.next_event())
    }
    /// Will attempt to immediately pull an event out of the queue
    ///
    /// # Errors
    ///
    /// Will return [`HotShotError::NetworkFault`] if the underlying [`HotShot`] instance has shut down
    pub fn try_next_event(&mut self) -> Result<Option<Event<I::Block, I::State, N>>> {
        let result = self.stream_output.try_recv();
        Ok(result)
    }

    /// Will pull all the currently available events out of the event queue.
    ///
    /// # Errors
    ///
    /// Will return [`HotShotError::NetworkFault`] if the underlying [`HotShot`] instance has been shut
    /// down.
    pub fn available_events(&mut self) -> Result<Vec<Event<I::Block, I::State, N>>> {
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
    pub async fn get_state(&self) -> Result<Option<I::State>> {
        self.hotshot.get_state().await
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
        tx: <<I as NodeImplementation<N>>::Block as BlockContents<N>>::Transaction,
    ) -> Result<()> {
        self.hotshot.publish_transaction_async(tx).await
    }

    /// Synchronously submits a transaction to the backing [`HotShot`] instance.
    ///
    /// # Errors
    ///
    /// See documentation for `submit_transaction`
    pub fn submit_transaction_sync(
        &self,
        tx: <<I as NodeImplementation<N>>::Block as BlockContents<N>>::Transaction,
    ) -> Result<()> {
        block_on(self.submit_transaction(tx))
    }

    /// Signals to the underlying [`HotShot`] to unpause
    ///
    /// This will cause the background task to start running consensus again.
    pub async fn start(&self) {
        self.hotshot.inner.background_task_handle.start().await;
    }

    /// Signals the underlying [`HotShot`] to run one round, if paused.
    ///
    /// Do not call this function if [`HotShot`] has been unpaused by [`HotShotHandle::start`].
    pub async fn run_one_round(&self) {
        self.hotshot
            .inner
            .background_task_handle
            .run_one_round()
            .await;
    }

    /// Synchronously signals the underlying [`HotShot`] to run one round, if paused
    pub fn run_one_round_sync(&self) {
        block_on(self.run_one_round());
    }

    /// iterate through all events on a [`NodeImplementation`] and determine if the node finished
    /// successfully
    /// # Errors
    /// Errors if unable to obtain storage
    pub async fn collect_round_events(&mut self) -> Result<(Vec<I::State>, Vec<I::Block>)> {

        // TODO we should probably do a view check
        // but we can do that later. It's non-obvious how to get the view number out
        // to check against

        // drain all events from this node
        loop {
            // unwrap is fine here since the thing hasn't been shut down
            let event = self.next_event().await.unwrap();
            match event.event {
                EventType::ViewTimeout { view_number } => {
                    error!(?event, "Round timed out!");
                    return Err(HotShotError::ViewTimeoutError {
                        view_number,
                        state: RoundTimedoutState::TestCollectRoundEventsTimedOut,
                    });
                }
                EventType::Decide { block, state, .. } => {
                    return Ok((
                        state.iter().cloned().collect(),
                        block.iter().cloned().collect(),
                    ));
                }
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

    /// Blocks until network is ready to be used (e.g. connected to other nodes)
    pub async fn is_ready(&self) -> bool {
        self.hotshot.inner.networking.ready().await
    }

    /// Shut down the the inner hotshot and wait until all background threads are closed.
    pub async fn shut_down(self) {
        self.shut_down.store(true, Ordering::Relaxed);
        self.hotshot.inner.networking.shut_down().await;
        self.hotshot
            .inner
            .background_task_handle
            .wait_shutdown()
            .await;
    }

    /// return the timeout for a view of the underlying `HotShot`
    pub fn get_next_view_timeout(&self) -> u64 {
        self.hotshot.get_next_view_timeout()
    }
}
