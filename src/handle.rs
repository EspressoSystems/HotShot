use async_std::{sync::RwLock, task::block_on};
use snafu::{ResultExt, Snafu};
use tokio::sync::broadcast::{self, RecvError, TryRecvError};

use std::sync::Arc;

use crate::{error::HotStuffError, event::Event, BlockContents, HotStuff};

/// Handle for interacting with a `HotStuff` instance
pub struct HotStuffHandle<B: BlockContents + 'static> {
    /// Handle to a sender for the output stream
    ///
    /// Kept around because we need to be able to call `subscribe` on it to generate new receivers
    pub(crate) sender_handle: Arc<broadcast::Sender<Event<B, B::State>>>,
    /// Internal `HotStuff` reference
    pub(crate) hotstuff: HotStuff<B>,
    /// The receiver we use to receive events on
    pub(crate) stream_output: broadcast::Receiver<Event<B, B::State>>,
    /// Global control to pause the underlying `HotStuff`
    pub(crate) pause: Arc<RwLock<bool>>,
    /// Override for the `pause` value that allows the `HotStuff` to run one round
    pub(crate) run_once: Arc<RwLock<bool>>,
    /// Global to signify the `HotStuff` should be closed after completing the next round
    pub(crate) shut_down: Arc<RwLock<bool>>,
}

impl<B: BlockContents + 'static> Clone for HotStuffHandle<B> {
    fn clone(&self) -> Self {
        Self {
            sender_handle: self.sender_handle.clone(),
            stream_output: self.sender_handle.subscribe(),
            hotstuff: self.hotstuff.clone(),
            pause: self.pause.clone(),
            run_once: self.run_once.clone(),
            shut_down: self.shut_down.clone(),
        }
    }
}

impl<B: BlockContents + 'static> HotStuffHandle<B> {
    /// Will return the next event in the queue
    ///
    /// # Errors
    ///
    /// - Will return `HandleError::Closed` if the underlying `HotStuff` has been closed.
    /// - Will return `HandleError::Skipped{ ammount }` if this receiver has fallen behind. `ammount`
    ///   indicates the number of messages that were skipped, and a subsequent call should succeed,
    ///   returning the oldest value still in queue.
    pub async fn next_event(&mut self) -> Result<Event<B, B::State>, HandleError> {
        let result = self.stream_output.recv().await;
        match result {
            Ok(result) => Ok(result),
            Err(RecvError::Closed) => Err(HandleError::ShutDown),
            Err(RecvError::Lagged(x)) => Err(HandleError::Skipped { ammount: x }),
        }
    }
    /// Syncronous version of `next_event`
    ///
    /// Will internally call `block_on` on `next_event`
    ///
    /// # Errors
    ///
    /// See documentation for `next_event`
    pub fn next_event_sync(&mut self) -> Result<Event<B, B::State>, HandleError> {
        block_on(self.next_event())
    }
    /// Will attempt to immediatly pull an event out of the queue
    ///
    /// # Errors
    ///
    /// - Will return `HandleError::ShutDown` if the underlying `HotStuff` instance has shut down
    /// - Will return `HandleError::Skipped{ ammount }` if this receiver has fallen behind. `ammount`
    ///   indicates the number of messages that were skipped, and a subsequent call should succeed,
    ///   returning the oldest value still in queue.
    pub fn try_next_event(&mut self) -> Result<Option<Event<B, B::State>>, HandleError> {
        let result = self.stream_output.try_recv();
        match result {
            Ok(result) => Ok(Some(result)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Closed) => Err(HandleError::ShutDown),
            Err(TryRecvError::Lagged(ammount)) => Err(HandleError::Skipped { ammount }),
        }
    }

    /// Will pull all the currently available events out of the event queue.
    ///
    /// This will ignore the case where the receiver has lagged behind, and discard the
    /// `HandleError::Skipped` message.
    ///
    /// # Errors
    ///
    /// Will return `HandleError::ShutDown` if the underlying `HotStuff` instance has been shut down.
    pub fn availible_events(&mut self) -> Result<Vec<Event<B, B::State>>, HandleError> {
        let mut output = vec![];
        // Loop to pull out all the outputs
        loop {
            match self.try_next_event() {
                Ok(Some(x)) => output.push(x),
                Ok(None) => break,
                Err(HandleError::Skipped { .. }) => continue,
                Err(HandleError::ShutDown) => return Err(HandleError::ShutDown),
                // As try_next event can only return HandleError::Skipped or HandleError::ShutDown,
                // it would be nonsensical if we end up here
                _ => unreachable!("Impossible to reach branch in HotStuffHandle::available_events"),
            }
        }
        Ok(output)
    }

    /// Gets the current commited state of the `HotStuff` instance.
    pub async fn get_state(&self) -> Arc<B::State> {
        self.hotstuff.get_state().await
    }

    /// Gets the current commited state of the `HotStuff` instance, blocking on the future
    pub fn get_state_sync(&self) -> Arc<B::State> {
        block_on(self.get_state())
    }

    /// Submits a transaction to the backing `HotStuff` instance.
    ///
    /// # Errors
    ///
    /// Will return a `HandleError::Transaction` if some error occurs in the underlying `HotStuff` instance.
    pub async fn submit_transaction(&self, tx: B::Transaction) -> Result<(), HandleError> {
        self.hotstuff
            .publish_transaction_async(tx)
            .await
            .context(Transaction)
    }

    /// Sycronously sumbits a transaction to the backing `HotStuff` instance.
    ///
    /// # Errors
    ///
    /// See documentation for `submit_transaction`
    pub fn submit_transaction_sync(&self, tx: B::Transaction) -> Result<(), HandleError> {
        block_on(self.submit_transaction(tx))
    }

    /// Signals to the underlying `HotStuff` to unpause
    pub async fn start(&self) {
        *self.pause.write().await = false;
    }

    /// Synchronously signals the underlying `HotStuff` to unpause
    pub fn start_sync(&self) {
        block_on(self.start());
    }

    /// Signals the underlying `HotStuff` to pause
    pub async fn pause(&self) {
        *self.pause.write().await = true;
    }

    /// Synchronously signals the underlying `HotStuff` to pause
    pub fn pause_sync(&self) {
        block_on(self.pause());
    }

    /// Signals the underlying `HotStuff` to run one round, if paused
    pub async fn run_one_round(&self) {
        let paused = self.pause.read().await;
        if *paused {
            *self.run_once.write().await = true;
        }
    }

    /// Synchronously signals the underlying `HotStuff` to run one round, if paused
    pub fn run_one_round_sync(&self) {
        block_on(self.run_one_round())
    }
}

/// Represents the types of errors that can be returned by a `HotStuffHandle`
#[derive(Snafu, Debug)]
#[allow(clippy::large_enum_variant)] // HotStuff error isn't that big, and these are _errors_ after all
pub enum HandleError {
    /// This handle has not had an event pulled out of it for too long, and some messages were
    /// skipped
    Skipped {
        /// The number of messages skipped
        ammount: u64,
    },
    /// The `HotStuff` instance this handle references has shut down
    ShutDown,
    /// An error occured in the underlying `HotStuff` implementation while submitting a transaction
    Transaction {
        /// The underlying `HotStuff` error
        source: HotStuffError,
    },
}
