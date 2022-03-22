//! Provides an event-streaming handle for a [`PhaseLock`] running in the background

use async_std::{sync::RwLock, task::block_on};
use std::sync::Arc;

use crate::{
    traits::{BlockContents, NetworkError::ShutDown, NodeImplementation},
    types::{Event, PhaseLockError, PhaseLockError::NetworkFault},
    PhaseLock,
};

use phaselock_utils::broadcast::{BroadcastReceiver, BroadcastSender};

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
    /// Global control to pause the underlying [`PhaseLock`]
    pub(crate) pause: Arc<RwLock<bool>>,
    /// Override for the `pause` value that allows the [`PhaseLock`] to run one round, before being
    /// repaused
    pub(crate) run_once: Arc<RwLock<bool>>,
    /// Global to signify the `PhaseLock` should be closed after completing the next round
    pub(crate) shut_down: Arc<RwLock<bool>>,
    /// Our copy of the `Storage` view for a phaselock
    pub(crate) storage: I::Storage,
}

impl<B: NodeImplementation<N> + 'static, const N: usize> Clone for PhaseLockHandle<B, N> {
    fn clone(&self) -> Self {
        Self {
            sender_handle: self.sender_handle.clone(),
            stream_output: self.sender_handle.handle_sync(),
            phaselock: self.phaselock.clone(),
            pause: self.pause.clone(),
            run_once: self.run_once.clone(),
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
    /// Will return [`PhaseLockError::NetworkError`] if the underlying [`PhaseLock`] has been closed.
    pub async fn next_event(&mut self) -> Result<Event<I::Block, I::State>, PhaseLockError> {
        let result = self.stream_output.recv_async().await;
        match result {
            Ok(result) => Ok(result),
            Err(_) => Err(NetworkFault { source: ShutDown }),
        }
    }
    /// Syncronous version of `next_event`
    ///
    /// Will internally call `block_on` on `next_event`
    ///
    /// # Errors
    ///
    /// See documentation for `next_event`
    pub fn next_event_sync(&mut self) -> Result<Event<I::Block, I::State>, PhaseLockError> {
        block_on(self.next_event())
    }
    /// Will attempt to immediatly pull an event out of the queue
    ///
    /// # Errors
    ///
    /// Will return [`PhaseLockError::NetworkError`] if the underlying [`PhaseLock`] instance has shut down
    pub fn try_next_event(&mut self) -> Result<Option<Event<I::Block, I::State>>, PhaseLockError> {
        let result = self.stream_output.try_recv();
        Ok(result)
    }

    /// Will pull all the currently available events out of the event queue.
    ///
    /// # Errors
    ///
    /// Will return [`PhaseLockError::NetworkError`] if the underlying [`PhaseLock`] instance has been shut
    /// down.
    pub fn availible_events(&mut self) -> Result<Vec<Event<I::Block, I::State>>, PhaseLockError> {
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

    /// Gets the current commited state of the [`PhaseLock`] instance
    pub async fn get_state(&self) -> Arc<I::State> {
        self.phaselock.get_state().await
    }

    /// Gets the current commited state of the [`PhaseLock`] instance, blocking on the future
    pub fn get_state_sync(&self) -> Arc<I::State> {
        block_on(self.get_state())
    }

    /// Submits a transaction to the backing [`PhaseLock`] instance.
    ///
    /// The current node broadcasts the transaction to all nodes on the network.
    ///
    /// # Errors
    ///
    /// Will return a [`PhaselockError`] if some error occurs in the underlying
    /// [`PhaseLock`] instance.
    pub async fn submit_transaction(
        &self,
        tx: <<I as NodeImplementation<N>>::Block as BlockContents<N>>::Transaction,
    ) -> Result<(), PhaseLockError> {
        self.phaselock.publish_transaction_async(tx).await
    }

    /// Sycronously sumbits a transaction to the backing [`PhaseLock`] instance.
    ///
    /// # Errors
    ///
    /// See documentation for `submit_transaction`
    pub fn submit_transaction_sync(
        &self,
        tx: <<I as NodeImplementation<N>>::Block as BlockContents<N>>::Transaction,
    ) -> Result<(), PhaseLockError> {
        block_on(self.submit_transaction(tx))
    }

    /// Signals to the underlying [`PhaseLock`] to unpause
    ///
    /// This will cause the background task to start running consensus again.
    pub async fn start(&self) {
        *self.pause.write().await = false;
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
        *self.pause.write().await = true;
    }

    /// Synchronously signals the underlying `PhaseLock` to pause
    pub fn pause_sync(&self) {
        block_on(self.pause());
    }

    /// Signals the underlying [`PhaseLock`] to run one round, if paused.
    ///
    /// Do not call this function if [`PhaseLock`] has been unpaused by [`PhaseLockHandle::start`].
    pub async fn run_one_round(&self) {
        let paused = self.pause.read().await;
        if *paused {
            *self.run_once.write().await = true;
        }
    }

    /// Synchronously signals the underlying [`PhaseLock`] to run one round, if paused
    pub fn run_one_round_sync(&self) {
        block_on(self.run_one_round());
    }

    /// Provides a reference to the underlying storage for this [`PhaseLock`], allowing access to
    /// historical data
    pub fn storage(&self) -> &I::Storage {
        &self.storage
    }
}
