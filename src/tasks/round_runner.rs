//! Round runner logic
//!
//! This will wait for one of several events, and will run one or multiple rounds

use crate::PhaseLock;
use async_std::{future::TimeoutError, task::JoinHandle};
use flume::{unbounded, Receiver, Sender};
use futures::channel::oneshot::Sender as OneShotSender;
use phaselock_types::{
    data::Stage,
    error::PhaseLockError,
    event::{Event, EventType},
    traits::{node_implementation::NodeImplementation, storage::Storage},
};
use std::{sync::Arc, time::Duration};
use tracing::{error, info, instrument, warn};

/// A round runner that will run one or multiple phaselock rounds.
///
/// This can be started and stopped by sending [`ToRoundRunner`] messages to the `sender` that is available on this struct.
pub struct RoundRunner<I: NodeImplementation<N>, const N: usize> {
    /// The join handle of the current running round
    join_handle: Option<JoinHandle<()>>,
    /// The sender that can be used to send messages to this round runner
    pub sender: Sender<ToRoundRunner>,
    /// The internal receiver used to receive messages from `sender`
    receiver: Receiver<ToRoundRunner>,
    /// The internal state of the round runner. This will be updated every time a round has finished.
    state: RoundRunnerState,
    /// A reference to the current running phaselock implementation.
    phaselock: PhaseLock<I, N>,
}

impl<I: NodeImplementation<N>, const N: usize> RoundRunner<I, N> {
    /// Create a new instance of the round runner.
    pub async fn new(phaselock: PhaseLock<I, N>) -> Self {
        let (sender, receiver) = unbounded();
        let view = match phaselock.inner.storage.get_newest_qc().await {
            Ok(Some(qc)) => qc.view_number,
            Ok(None) => 0,
            Err(e) => {
                error!(?e, "Could not load the newest QC from the storage. Assuming there are no QC in the system.");
                0
            }
        };
        let state = RoundRunnerState {
            view,
            int_duration: phaselock.inner.config.next_view_timeout,
            is_running: false,
        };
        Self {
            join_handle: None,
            sender,
            receiver,
            state,
            phaselock,
        }
    }

    /// Consume this round runner and run until it receives a `ToRoundRunner::Shutdown`.
    #[instrument(skip(self), fields(id = self.phaselock.inner.public_key.nonce))]
    pub async fn run(mut self) {
        loop {
            let message = match self.receiver.recv_async().await {
                Ok(message) => message,
                Err(e) => {
                    error!(
                        ?e,
                        "Could not receive message, did all senders get dropped?"
                    );
                    break;
                }
            };

            match message {
                ToRoundRunner::GetState(sender) => {
                    if let Err(e) = sender.send(self.state.clone()) {
                        error!(?e, "Could not notify parent of state");
                    }
                }
                ToRoundRunner::NewViewNumber(view) => {
                    if self.join_handle.is_some() {
                        warn!("Incoming new view number but a round is running. This will not work as expected.");
                    }
                    self.state.view = view;
                }
                ToRoundRunner::Pause => {
                    self.state.is_running = false;
                }
                ToRoundRunner::Run => {
                    self.state.is_running = true;
                    if self.join_handle.is_none() && !self.spawn().await {
                        break;
                    }
                }
                ToRoundRunner::RunOnce => {
                    if self.join_handle.is_none() {
                        if !self.spawn().await {
                            break;
                        }
                    } else {
                        warn!("Received `RunOnce` command but a round is already running. The command is ignored.");
                    }
                }
                ToRoundRunner::ShutDown => {
                    info!("Round runner shut down gracefully");
                    return;
                }
                ToRoundRunner::RoundFinished(result) => {
                    if async_std::future::timeout(
                        Duration::from_millis(100),
                        self.join_handle.take().unwrap(),
                    )
                    .await
                    .is_err()
                    {
                        error!("Could not wait for the handle to join after it reportedly finished. This is a bug.");
                        break;
                    }
                    match *result {
                        Ok(Ok(new_view)) => {
                            info!("Round finished, new view number is {}", new_view);
                            if self.state.is_running && !self.spawn().await {
                                break;
                            }
                        }
                        Err(_) => {
                            warn!("Round timed out");
                        }
                        Ok(Err(e)) => {
                            warn!(?e, "Could not finish round");
                        }
                    }
                }
            }
        }
        error!("Round runner shut down incorrectly. This is a bug.");
    }

    /// Spawn a single round. This will set `self.join_handle`.
    ///
    /// Will return `true` if the round was successfully spawned. If this returns `false`, the caller should shut down.
    async fn spawn(&mut self) -> bool {
        // Send the next view
        let next_view_res = self.phaselock.next_view(self.state.view).await;
        // If we fail to send the next view, broadcast the error and pause
        if let Err(e) = next_view_res {
            if !self
                .phaselock
                .send_event(Event {
                    view_number: self.state.view,
                    stage: e.get_stage().unwrap_or(Stage::None),

                    event: EventType::Error { error: Arc::new(e) },
                })
                .await
            {
                error!("All event streams closed! Shutting down.");
                return false;
            }
            self.state.is_running = false;
            // Not successful but we can try again later
            return true;
        }
        // Increment the view counter
        self.state.view += 1;
        // run the next block, with a timeout
        let t = Duration::from_millis(self.state.int_duration);

        assert!(self.join_handle.is_none());
        self.join_handle = Some(async_std::task::spawn({
            let sender = self.sender.clone();
            let phaselock = self.phaselock.clone();
            let view = self.state.view;
            async move {
                let round_res = async_std::future::timeout(t, phaselock.run_round(view)).await;
                if let Err(e) = sender.send(ToRoundRunner::RoundFinished(Box::new(round_res))) {
                    error!(?e, "Could not send round result");
                }
            }
        }));
        true
    }
}

/// The internal state of the [`round_runner_task`].
#[derive(Debug, PartialEq, Clone)]
pub struct RoundRunnerState {
    /// The view number of the next `QuorumCertificate`
    pub view: u64,

    /// The timeout of the next round.
    pub int_duration: u64,

    /// `true` if the background runner is running constantly
    pub is_running: bool,
}

/// Events going to the round runner.
pub enum ToRoundRunner {
    /// Notify the round runner that there is a new view number inserted externally that it should use from now on.
    NewViewNumber(u64),

    /// Request the current state of the round runner.
    GetState(OneShotSender<RoundRunnerState>),

    /// Pause the runner
    Pause,

    /// Start the runner. Will run indefinitely
    Run,

    /// Run one round
    RunOnce,

    /// Shut down the runner
    ShutDown,

    /// Will be triggered once a round is done
    RoundFinished(Box<Result<Result<u64, PhaseLockError>, TimeoutError>>),
}
