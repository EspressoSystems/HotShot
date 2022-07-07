//! Round runner logic
//!
//! This will wait for one of several events, and will run one or multiple rounds

use crate::PhaseLock;
use async_std::{prelude::FutureExt, task::JoinHandle};
use flume::{unbounded, Receiver, Sender};
use futures::channel::oneshot::{self, Sender as OneShotSender};
use phaselock_types::{
    data::{Stage, ViewNumber},
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

    /// Counter of how many rounds need to be run. This allows us to send multiple `RunOnce` commands and the backround runner will handle this correctly.
    run_once_counter: usize,

    /// The timeout of the next round.
    int_duration: u64,

    /// The amount of times in a row that a round timed out
    round_timeout_seq_count: usize,
}

impl<I: NodeImplementation<N>, const N: usize> RoundRunner<I, N> {
    /// Create a new instance of the round runner.
    pub async fn new(phaselock: PhaseLock<I, N>) -> Self {
        let (sender, receiver) = unbounded();
        let view = match phaselock.inner.storage.get_newest_qc().await {
            Ok(Some(qc)) => qc.view_number,
            Ok(None) => ViewNumber::genesis(),
            Err(e) => {
                error!(?e, "Could not load the newest QC from the storage. Assuming there are no QC in the system.");
                ViewNumber::genesis()
            }
        };
        let state = RoundRunnerState {
            view,
            is_running: false,
        };
        let int_duration = phaselock.inner.config.next_view_timeout;
        Self {
            join_handle: None,
            sender,
            receiver,
            state,
            phaselock,
            int_duration,
            run_once_counter: 0,
            round_timeout_seq_count: 0,
        }
    }

    /// Consume this round runner and run until it receives a `ToRoundRunner::Shutdown`.
    #[instrument(skip(self))]
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
                    tracing::debug!(?self.state, "Current state");
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
                        info!("Received `RunOnce` command but a round is already running. Will schedule `run_once_counter`");
                        self.run_once_counter += 1;
                    }
                }
                ToRoundRunner::ShutDown => {
                    info!("Round runner shut down gracefully");
                    return;
                }
                ToRoundRunner::RoundFinished(result) => {
                    tracing::debug!(?result, "Round finished");
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

                    let default_interrupt_duration = self.phaselock.inner.config.next_view_timeout;
                    let (int_mul, int_div) = self.phaselock.inner.config.timeout_ratio;

                    let mut event_to_send = None;
                    match *result {
                        Ok(new_view) => {
                            // If it succeded, simply reset the timeout
                            self.int_duration = default_interrupt_duration;
                            self.round_timeout_seq_count = 0;

                            info!("Round finished, new view number is {:?}", new_view);
                        }
                        Err(PhaseLockError::ViewTimeoutError { view_number, state }) => {
                            if view_number != self.state.view {
                                error!("We received a timeout for view {:?} but we're currently in view {:?}", view_number, self.state.view);
                            }
                            self.round_timeout_seq_count += 1;
                            if self.round_timeout_seq_count <= 2 {
                                info!(
                                    "Round timed out ({} times). This is fine. State = {:?}",
                                    self.round_timeout_seq_count, state
                                );
                            } else if self.round_timeout_seq_count <= 5 {
                                warn!(
                                    "Round timed out ({} times), state = {:?}",
                                    self.round_timeout_seq_count, state
                                );
                            } else {
                                error!("Round timed out {} times, we're probably not properly connected to the network any more, state = {:?}", self.round_timeout_seq_count, state);
                            }

                            event_to_send = Some(Event {
                                view_number: self.state.view,
                                stage: Stage::None,
                                event: EventType::ViewTimeout {
                                    view_number: self.state.view,
                                },
                            });

                            self.int_duration = (self.int_duration * int_mul) / int_div;
                        }
                        Err(e) => {
                            // If it errored, broadcast the error, reset the timeout, and continue
                            error!(?e, "Round encountered an error");
                            event_to_send = Some(Event {
                                view_number: self.state.view,
                                stage: e.get_stage().unwrap_or(Stage::None),
                                event: EventType::Error { error: Arc::new(e) },
                            });
                            self.int_duration = default_interrupt_duration;
                            self.round_timeout_seq_count = 0;
                        }
                    }

                    if let Some(event_to_send) = event_to_send {
                        if !self.phaselock.send_event(event_to_send).await {
                            error!("All event streams closed! Shutting down.");
                            break;
                        }
                    }

                    let should_start_new_round = if self.run_once_counter > 0 {
                        self.run_once_counter -= 1;
                        true
                    } else {
                        self.state.is_running
                    };

                    if should_start_new_round && !self.spawn().await {
                        break;
                    }
                }
                ToRoundRunner::RoundShouldBeFinished(view) => {
                    // We mark this round as timed out. If the round is not running then this is a no-op, otherwise we will be getting a `RoundFinished` soon.
                    self.phaselock.mark_round_as_timed_out(view).await;
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
        let next_view_res = self.phaselock.next_view(self.state.view + 1).await;
        // If we fail to send the next view, broadcast the error and pause
        if let Err(e) = next_view_res {
            if !self
                .phaselock
                .send_event(Event {
                    view_number: self.state.view + 1,
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
        tracing::debug!("New view number is now {:?}", self.state.view);

        let (round_finished_sender, round_finished_receiver) = oneshot::channel();

        assert!(self.join_handle.is_none());
        self.join_handle = Some(async_std::task::spawn({
            let sender = self.sender.clone();
            let phaselock = self.phaselock.clone();
            let view = self.state.view;
            async move {
                let round_res = phaselock.run_round(view).await;
                let _ = round_finished_sender.send(());
                if let Err(e) = sender.send(ToRoundRunner::RoundFinished(Box::new(round_res))) {
                    error!(?e, "Could not send round result");
                }
            }
        }));

        // spawn a fire-and-forget task that will notify us if a round should be timed out, unless our oneshot channel is triggered.
        async_std::task::spawn({
            let sender = self.sender.clone();
            let t = Duration::from_millis(self.int_duration);
            let view = self.state.view;
            async move {
                if round_finished_receiver.timeout(t).await.is_err() {
                    // If we weren't notified of `phaselock.rund_round` being finished, we send an interrupt
                    let _result = sender.send(ToRoundRunner::RoundShouldBeFinished(view));
                }
            }
        });
        true
    }
}

/// The internal state of the [`round_runner_task`].
#[derive(Debug, PartialEq, Clone)]
pub struct RoundRunnerState {
    /// The view number of the next `QuorumCertificate`
    pub view: ViewNumber,

    /// `true` if the background runner is running constantly
    pub is_running: bool,
}

/// Events going to the round runner.
pub enum ToRoundRunner {
    /// Notify the round runner that there is a new view number inserted externally that it should use from now on.
    NewViewNumber(ViewNumber),

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
    RoundFinished(Box<Result<ViewNumber, PhaseLockError>>),

    /// Will be send a fixed amount of time after a round started, to notify us that a round should be marked as timed out if it's not finished yet
    RoundShouldBeFinished(ViewNumber),
}
