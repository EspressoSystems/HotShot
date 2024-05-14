use anyhow::Result;
use async_broadcast::{Receiver, Sender};
#[cfg(async_executor_impl = "async-std")]
use async_std::{
    sync::RwLock,
    task::{spawn, JoinHandle},
};
use async_trait::async_trait;
#[cfg(async_executor_impl = "async-std")]
use futures::future::join_all;
#[cfg(async_executor_impl = "tokio")]
use futures::future::try_join_all;
#[cfg(async_executor_impl = "tokio")]
use tokio::{
    sync::RwLock,
    task::{spawn, JoinHandle},
};

#[async_trait]
/// Type for mutable task state that can be used as the state for a `Task`
pub trait TaskState: Send {
    /// Type of event sent and received by the task
    type Event: Clone + Send + Sync;
    /// Handles an event, possibly mutating our state.
    /// Returns a sequence of events to broadcast to the channel,
    /// or an error trace in handling the message.
    async fn handle_event(&mut self, _event: Self::Event) -> Result<Vec<Self::Event>> {
        Ok(vec![])
    }

    /// Handles an event, providing direct access to the specific channel we received the event on.
    ///
    /// If possible, a task should prefer to implement `handle_event` rather than `handle_event_direct`.
    /// A long-term goal would be to deprecate `handle_event_direct` in favor of `handle_event`, possibly with a more general return type.
    async fn handle_event_direct(
        &mut self,
        event: Self::Event,
        _sender: &Sender<Self::Event>,
        _receiver: &Receiver<Self::Event>,
    ) -> Result<Vec<Self::Event>> {
        self.handle_event(event).await
    }
}

/// A basic task which loops waiting for events to come from `event_receiver`
/// and then handles them using it's state
/// It sends events to other `Task`s through `event_sender`
/// This should be used as the primary building block for long running
/// or medium running tasks (i.e. anything that can't be described as a dependency task)
pub struct Task<S: TaskState> {
    /// The state of the task.  It is fed events from `event_sender`
    /// and mutates it state ocordingly.  Also it signals the task
    /// if it is complete/should shutdown
    state: S,
    /// Whether an event should cause the task to return, given as a boolean predicate.
    break_on: fn(&S::Event) -> bool,
    /// Sends events all tasks including itself
    sender: Sender<S::Event>,
    /// Receives events that are broadcast from any task, including itself
    receiver: Receiver<S::Event>,
}

impl<S: TaskState + Send + 'static> Task<S> {
    /// Create a new task
    pub fn new(
        state: S,
        break_on: fn(&S::Event) -> bool,
        sender: Sender<S::Event>,
        receiver: Receiver<S::Event>,
    ) -> Self {
        Task {
            state,
            break_on,
            sender,
            receiver,
        }
    }

    /// The state of the task, as a boxed dynamic trait object.
    fn boxed_state(self) -> Box<dyn TaskState<Event = S::Event>> {
        Box::new(self.state) as Box<dyn TaskState<Event = S::Event>>
    }

    /// Spawn the task loop, consuming self.  Will continue until
    /// the task reaches some shutdown condition
    pub fn run(mut self) -> JoinHandle<Box<dyn TaskState<Event = S::Event>>> {
        spawn(async move {
            loop {
                match self.receiver.recv_direct().await {
                    Ok(input) => {
                        if (self.break_on)(&input) {
                            break self.boxed_state();
                        }

                        match S::handle_event_direct(
                            &mut self.state,
                            input,
                            &self.sender,
                            &self.receiver,
                        )
                        .await
                        {
                            Ok(outputs) => {
                                for output in outputs {
                                    let _ = self.sender.broadcast_direct(output).await.inspect_err(
                                        |e| {
                                            tracing::error!("{e}");
                                        },
                                    );
                                }
                            }
                            Err(e) => tracing::info!("{e}"),
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to receive from event stream Error: {}", e);
                    }
                }
            }
        })
    }
}

/// A collection of tasks which can handle shutdown
pub struct TaskRegistry<EVENT> {
    /// Tasks this registry controls
    task_handles: RwLock<Vec<JoinHandle<Box<dyn TaskState<Event = EVENT>>>>>,
}

impl<EVENT> TaskRegistry<EVENT> {
    /// Create a new task registry
    pub fn new() -> Self {
        TaskRegistry {
            task_handles: RwLock::new(vec![]),
        }
    }
    /// Add a task to the registry
    pub async fn register(&self, handle: JoinHandle<Box<dyn TaskState<Event = EVENT>>>) {
        self.task_handles.write().await.push(handle);
    }
    /// Try to cancel/abort the task this registry has
    pub async fn shutdown(&self) {
        let mut handles = self.task_handles.write().await;
        while let Some(handle) = handles.pop() {
            #[cfg(async_executor_impl = "async-std")]
            handle.cancel().await;
            #[cfg(async_executor_impl = "tokio")]
            handle.abort();
        }
    }
    /// Take a task, run it, and register it
    pub async fn run_task<S>(&self, task: Task<S>)
    where
        S: TaskState<Event = EVENT> + Send + 'static,
    {
        self.register(task.run()).await;
    }

    /// Wait for the results of all the tasks registered
    /// # Panics
    /// Panics if one of the tasks panicked
    pub async fn join_all(self) -> Vec<Box<dyn TaskState<Event = EVENT>>> {
        #[cfg(async_executor_impl = "async-std")]
        let ret = join_all(self.task_handles.into_inner()).await;
        #[cfg(async_executor_impl = "tokio")]
        let ret = try_join_all(self.task_handles.into_inner()).await.unwrap();
        ret
    }
}
