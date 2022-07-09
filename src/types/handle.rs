//! Provides an event-streaming handle for a [`PhaseLock`] running in the background

use crate::{
    tasks::RoundRunnerState,
    traits::{BlockContents, NetworkError::ShutDown, NodeImplementation},
    types::{Event, PhaseLockError::NetworkFault},
    PhaseLock, Result,
};
use async_std::{prelude::FutureExt, task::block_on};
use phaselock_types::{
    error::{PhaseLockError, RoundTimedoutState, TimeoutSnafu},
    event::EventType,
    traits::{network::NetworkingImplementation, storage::StorageError},
};
use phaselock_utils::broadcast::{BroadcastReceiver, BroadcastSender};
use snafu::ResultExt;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tracing::{debug, error};

/// Event streaming handle for a [`PhaseLock`] instance running in the background
///
/// This type provides the means to message and interact with a background [`PhaseLock`] instance,
/// allowing the ability to receive [`Event`]s from it, send transactions to it, and interact with
/// the underlying storage.
pub struct PhaseLockHandle<I: NodeImplementation<N> + Send + Sync + 'static, const N: usize> {
    /// The [sender](BroadcastSender) for the output stream from the background process
    ///
    /// This is kept around as an implementation detail, as the [`BroadcastSender::handle_async`]
    /// method is needed to generate new receivers for cloning the handle.
    pub(crate) sender_handle: Arc<BroadcastSender<Event<I::Block, I::State>>>,
    /// Internal reference to the underlying [`PhaseLock`]
    pub(crate) phaselock: PhaseLock<I, N>,
    /// The [`BroadcastReceiver`] we get the events from
    pub(crate) stream_output: BroadcastReceiver<Event<I::Block, I::State>>,
    /// Global to signify the `PhaseLock` should be closed after completing the next round
    pub(crate) shut_down: Arc<AtomicBool>,
    /// Our copy of the `Storage` view for a phaselock
    pub(crate) storage: I::Storage,
}

impl<B: NodeImplementation<N> + 'static, const N: usize> Clone for PhaseLockHandle<B, N> {
    fn clone(&self) -> Self {
        Self {
            sender_handle: self.sender_handle.clone(),
            stream_output: self.sender_handle.handle_sync(),
            phaselock: self.phaselock.clone(),
            shut_down: self.shut_down.clone(),
            storage: self.storage.clone(),
        }
    }
}

impl<I: NodeImplementation<N> + 'static, const N: usize> PhaseLockHandle<I, N> {
    /// Will return the next event in the queue
    ///
    /// # Errors
    ///
    /// Will return [`PhaseLockError::NetworkFault`] if the underlying [`PhaseLock`] has been closed.
    pub async fn next_event(&mut self) -> Result<Event<I::Block, I::State>> {
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
    pub fn next_event_sync(&mut self) -> Result<Event<I::Block, I::State>> {
        block_on(self.next_event())
    }
    /// Will attempt to immediately pull an event out of the queue
    ///
    /// # Errors
    ///
    /// Will return [`PhaseLockError::NetworkFault`] if the underlying [`PhaseLock`] instance has shut down
    pub fn try_next_event(&mut self) -> Result<Option<Event<I::Block, I::State>>> {
        let result = self.stream_output.try_recv();
        Ok(result)
    }

    /// Will pull all the currently available events out of the event queue.
    ///
    /// # Errors
    ///
    /// Will return [`PhaseLockError::NetworkFault`] if the underlying [`PhaseLock`] instance has been shut
    /// down.
    pub fn available_events(&mut self) -> Result<Vec<Event<I::Block, I::State>>> {
        let mut output = vec![];
        // Loop to pull out all the outputs
        loop {
            match self.try_next_event() {
                Ok(Some(x)) => output.push(x),
                Ok(None) => break,
                // // try_next event can only return PhaseLockError { source: NetworkError::ShutDown }
                Err(x) => return Err(x),
            }
        }
        Ok(output)
    }

    /// Gets the current committed state of the [`PhaseLock`] instance
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying `Storage` returns an error
    pub async fn get_state(&self) -> Result<Option<I::State>> {
        self.phaselock.get_state().await
    }

    /// Submits a transaction to the backing [`PhaseLock`] instance.
    ///
    /// The current node broadcasts the transaction to all nodes on the network.
    ///
    /// # Errors
    ///
    /// Will return a [`PhaseLockError`] if some error occurs in the underlying
    /// [`PhaseLock`] instance.
    pub async fn submit_transaction(
        &self,
        tx: <<I as NodeImplementation<N>>::Block as BlockContents<N>>::Transaction,
    ) -> Result<()> {
        self.phaselock.publish_transaction_async(tx).await
    }

    /// Synchronously submits a transaction to the backing [`PhaseLock`] instance.
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

    /// Signals to the underlying [`PhaseLock`] to unpause
    ///
    /// This will cause the background task to start running consensus again.
    pub async fn start(&self) {
        self.phaselock.inner.background_task_handle.start().await;
    }

    /// Synchronously signals the underlying [`PhaseLock`] to unpause
    pub fn start_sync(&self) {
        block_on(self.start());
    }

    /// Signals the underlying [`PhaseLock`] to pause
    ///
    /// This will cause the background task to stop driving consensus after the completion of the
    /// current view.
    pub async fn pause(&self) {
        self.phaselock.inner.background_task_handle.pause().await;
    }

    /// Synchronously signals the underlying `PhaseLock` to pause
    pub fn pause_sync(&self) {
        block_on(self.pause());
    }

    /// Signals the underlying [`PhaseLock`] to run one round, if paused.
    ///
    /// Do not call this function if [`PhaseLock`] has been unpaused by [`PhaseLockHandle::start`].
    pub async fn run_one_round(&self) {
        self.phaselock
            .inner
            .background_task_handle
            .run_one_round()
            .await;
    }

    /// Synchronously signals the underlying [`PhaseLock`] to run one round, if paused
    pub fn run_one_round_sync(&self) {
        block_on(self.run_one_round());
    }

    /// iterate through all events on a [`NodeImplementation`] and determine if the node finished
    /// successfully
    /// # Errors
    /// Errors if unable to obtain storage
    pub async fn collect_round_events(&mut self) -> Result<(Vec<I::State>, Vec<I::Block>)> {
        let cur_view = self
            .get_round_runner_state()
            .await
            .map_err(|e| PhaseLockError::StorageError {
                source: StorageError::InconsistencyError {
                    description: e.to_string(),
                },
            })?
            .view;

        // timeout for first event is longer in case
        // there is a delta before other nodes are spun up
        let mut timeout = Duration::from_secs(10);

        // drain all events from this node
        loop {
            let event = self
                .next_event()
                .timeout(timeout)
                .await
                .context(TimeoutSnafu)??;
            timeout = Duration::from_millis(self.get_next_view_timeout());
            match event.event {
                EventType::ViewTimeout { view_number } => {
                    if view_number >= cur_view {
                        error!(?event, "Round timed out!");
                        return Err(PhaseLockError::ViewTimeoutError {
                            view_number,
                            state: RoundTimedoutState::TestCollectRoundEventsTimedOut,
                        });
                    }
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

    /// Provides a reference to the underlying storage for this [`PhaseLock`], allowing access to
    /// historical data
    pub fn storage(&self) -> &I::Storage {
        &self.storage
    }

    /// Provides a reference to the underlying networking interface for this [`PhaseLock`], allowing access to
    /// networking stats.
    pub fn networking(&self) -> &I::Networking {
        &self.phaselock.inner.networking
    }

    /// Blocks until network is ready to be used (e.g. connected to other nodes)
    pub async fn is_ready(&self) -> bool {
        self.phaselock.inner.networking.ready().await
    }

    /// Shut down the the inner phaselock and wait until all background threads are closed.
    pub async fn shut_down(self) {
        self.shut_down.store(true, Ordering::Relaxed);
        self.phaselock.inner.networking.shut_down().await;
        self.phaselock
            .inner
            .background_task_handle
            .wait_shutdown()
            .await;
    }

    /// Get the state of the internal round runner. This is used for testing purposes.
    ///
    /// # Errors
    ///
    /// Will return an error when the background thread is unable to respond within a second.
    pub async fn get_round_runner_state(
        &self,
    ) -> std::result::Result<RoundRunnerState, Box<dyn std::error::Error>> {
        self.phaselock
            .inner
            .background_task_handle
            .get_round_runner_state()
            .await
    }

    /// return the timeout for a view of the underlying `Phaselock`
    pub fn get_next_view_timeout(&self) -> u64 {
        self.phaselock.get_next_view_timeout()
    }
}
