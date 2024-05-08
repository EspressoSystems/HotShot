use anyhow::Result;
use async_broadcast::{Receiver, SendError, Sender};
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
    type Event: Clone + Send + Sync + 'static;
    /// Handle event and update state.  Return true if the task is finished
    /// false otherwise.  The handler can access the state through `Task::state_mut`
    async fn handle_event(self: &mut Self, event: Self::Event) -> Result<()>;

    /// Whether the task should break on the given event.
    fn break_on(self: &Self, event: &Self::Event) -> bool;
}

/// A basic task which loops waiting for events to come from `event_receiver`
/// and then handles them using it's state
/// It sends events to other `Task`s through `event_sender`
/// This should be used as the primary building block for long running
/// or medium running tasks (i.e. anything that can't be described as a dependency task)
pub struct Task<S: TaskState> {
    /// Sends events all tasks including itself
    event_sender: Sender<S::Event>,
    /// Receives events that are broadcast from any task, including itself
    event_receiver: Receiver<S::Event>,
    /// The state of the task.  It is fed events from `event_sender`
    /// and mutates it state ocordingly.  Also it signals the task
    /// if it is complete/should shutdown
    state: S,
}

impl<S: TaskState + Send + 'static> Task<S> {
    /// Create a new task
    pub fn new(tx: Sender<S::Event>, rx: Receiver<S::Event>, state: S) -> Self {
        Task {
            event_sender: tx,
            event_receiver: rx,
            state,
        }
    }

    fn boxed_state(self) -> Box<dyn TaskState<Event = S::Event>> {
        Box::new(self.state) as Box<dyn TaskState<Event = S::Event>>
    }

    /// Spawn the task loop, consuming self.  Will continue until
    /// the task reaches some shutdown condition
    pub fn run(mut self) -> JoinHandle<Box<dyn TaskState<Event = S::Event>>> {
        spawn(async move {
            loop {
                match self.event_receiver.recv_direct().await {
                    Ok(event) => {
                        if self.state.break_on(&event) {
                            break self.boxed_state();
                        }

                        let _ = S::handle_event(self.state_mut(), event)
                            .await
                            .inspect_err(|e| tracing::info!("{e}"));
                    }
                    Err(e) => {
                        tracing::error!("Failed to receive from event stream Error: {}", e);
                    }
                }
            }
        })
    }

    /// Create a new event `Receiver` from this Task's receiver.
    /// The returned receiver will get all messages not yet seen by this task
    pub fn subscribe(&self) -> Receiver<S::Event> {
        self.event_receiver.clone()
    }
    /// Get a new sender handle for events
    pub fn sender(&self) -> &Sender<S::Event> {
        &self.event_sender
    }
    /// Clone the sender handle
    pub fn clone_sender(&self) -> Sender<S::Event> {
        self.event_sender.clone()
    }
    /// Broadcast a message to all listening tasks
    /// # Errors
    /// Errors if the broadcast fails
    pub async fn send(&self, event: S::Event) -> Result<Option<S::Event>, SendError<S::Event>> {
        self.event_sender.broadcast(event).await
    }
    /// Get a mutable reference to this tasks state
    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }
    /// Get an immutable reference to this tasks state
    pub fn state(&self) -> &S {
        &self.state
    }
}

#[derive(Default)]
/// A collection of tasks which can handle shutdown
pub struct TaskRegistry<EVENT> {
    /// Tasks this registry controls
    task_handles: RwLock<Vec<JoinHandle<Box<dyn TaskState<Event = EVENT>>>>>,
}

impl<EVENT> TaskRegistry<EVENT> {
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
