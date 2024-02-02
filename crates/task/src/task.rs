use std::sync::Arc;
use std::time::Duration;

use async_broadcast::{Receiver, SendError, Sender};
use async_compatibility_layer::art::async_timeout;
#[cfg(async_executor_impl = "async-std")]
use async_std::{
    sync::RwLock,
    task::{spawn, JoinHandle},
};
use futures::{future::select_all, Future};

#[cfg(async_executor_impl = "async-std")]
use futures::future::join_all;

#[cfg(async_executor_impl = "tokio")]
use futures::future::try_join_all;

#[cfg(async_executor_impl = "tokio")]
use tokio::{
    sync::RwLock,
    task::{spawn, JoinHandle},
};

use crate::{
    dependency::Dependency,
    dependency_task::{DependencyTask, HandleDepResult},
};

pub trait TaskState: Send {
    type Event: Clone + Send + Sync + 'static;
    type Result: Send;
    /// Handle event and update state.  Return true if the task is finished
    /// false otherwise
    fn handle_event(
        event: Self::Event,
        task: &mut Task<Self>,
    ) -> impl Future<Output = Option<Self::Result>> + Send
    where
        Self: Sized;

    /// Return true if the event should be filtered
    fn filter(&self, _event: &Self::Event) -> bool {
        // default doesn't filter
        false
    }
    /// Do something with the result of the task before it shuts down
    fn handle_result(&self, _res: &Self::Result) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }
    /// Return true if the event should shut the task down
    fn should_shutdown(event: &Self::Event) -> bool;
    /// Handle anything before the task is completely shutdown
    fn shutdown(&mut self) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }
}

pub trait TestTaskState: Send {
    type Message: Clone + Send + Sync + 'static;
    type Result: Send;
    type State: TaskState;
    fn handle_message(
        message: Self::Message,
        id: usize,
        task: &mut TestTask<Self::State, Self>,
    ) -> impl Future<Output = Option<Self::Result>> + Send
    where
        Self: Sized;
}

pub struct Task<S: TaskState> {
    event_sender: Sender<S::Event>,
    event_receiver: Receiver<S::Event>,
    registry: Arc<TaskRegistry>,
    state: S,
}

impl<S: TaskState + Send + 'static> Task<S> {
    pub fn new(
        tx: Sender<S::Event>,
        rx: Receiver<S::Event>,
        registry: Arc<TaskRegistry>,
        state: S,
    ) -> Self {
        Task {
            event_sender: tx,
            event_receiver: rx,
            registry,
            state,
        }
    }
    pub fn run(mut self) -> JoinHandle<()> {
        spawn(async move {
            loop {
                let event = self.event_receiver.recv_direct().await;
                if S::should_shutdown(event.as_ref().unwrap()) {
                    self.state.shutdown().await;
                    break;
                }
                if self.state.filter(event.as_ref().unwrap()) {
                    continue;
                }
                if let Some(res) = S::handle_event(event.unwrap(), &mut self).await {
                    self.state.handle_result(&res).await;
                    self.state.shutdown().await;
                    break;
                }
            }
        })
    }
    pub fn subscribe(&self) -> Receiver<S::Event> {
        self.event_receiver.clone()
    }
    pub fn sender(&self) -> &Sender<S::Event> {
        &self.event_sender
    }
    pub fn clone_sender(&self) -> Sender<S::Event> {
        self.event_sender.clone()
    }
    pub async fn send(&self, event: S::Event) -> Result<Option<S::Event>, SendError<S::Event>> {
        self.event_sender.broadcast(event).await
    }
    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }
    pub async fn run_sub_task(&self, state: S) {
        let task = Task {
            event_sender: self.clone_sender(),
            event_receiver: self.subscribe(),
            registry: self.registry.clone(),
            state,
        };
        // Note: await here is only awaiting the task to be added to the
        // registry, not for the task to run.
        self.registry.run_task(task).await;
    }
}

pub struct TestTask<S: TaskState, T: TestTaskState + Send> {
    task: Task<S>,
    message_receivers: Vec<Receiver<T::Message>>,
}

impl<
        S: TaskState + Send + 'static,
        T: TestTaskState<State = S, Result = S::Result> + Send + Sync + 'static,
    > TestTask<S, T>
{
    pub fn new(task: Task<S>, rxs: Vec<Receiver<T::Message>>) -> Self {
        Self {
            task,
            message_receivers: rxs,
        }
    }
    pub fn run(mut self) -> JoinHandle<S::Result> {
        spawn(async move {
            loop {
                let mut futs = vec![];

                if let Ok(event) = self.task.event_receiver.try_recv() {
                    if S::should_shutdown(&event) {
                        self.task.state.shutdown().await;
                        tracing::error!("Shutting down test task TODO!");
                        todo!();
                    }
                    if !self.state().filter(&event) {
                        if let Some(res) = S::handle_event(event, &mut self.task).await {
                            self.task.state.handle_result(&res).await;
                            self.task.state.shutdown().await;
                            return res;
                        }
                    }
                }

                for rx in self.message_receivers.iter_mut() {
                    futs.push(rx.recv());
                }
                if let Ok((Ok(msg), id, _)) =
                    async_timeout(Duration::from_secs(1), select_all(futs)).await
                {
                    if let Some(res) = T::handle_message(msg, id, &mut self).await {
                        self.task.state.handle_result(&res).await;
                        self.task.state.shutdown().await;
                        return res;
                    }
                }
            }
        })
    }
    pub fn state(&self) -> &S {
        &self.task.state
    }
    pub fn state_mut(&mut self) -> &mut S {
        self.task.state_mut()
    }
    pub async fn send_event(&self, event: S::Event) {
        self.task.send(event).await.unwrap();
    }
}

#[derive(Default)]
pub struct TaskRegistry {
    task_handles: RwLock<Vec<JoinHandle<()>>>,
}

impl TaskRegistry {
    pub async fn register(&self, handle: JoinHandle<()>) {
        self.task_handles.write().await.push(handle);
    }
    pub async fn shutdown(&self) {
        let mut handles = self.task_handles.write().await;
        while let Some(handle) = handles.pop() {
            #[cfg(async_executor_impl = "async-std")]
            handle.cancel().await;
            #[cfg(async_executor_impl = "tokio")]
            handle.abort();
        }
        tracing::error!("shut down all tasks");
    }
    pub async fn run_task<S>(&self, task: Task<S>)
    where
        S: TaskState + Send + 'static,
    {
        self.register(task.run()).await;
    }
    pub async fn spawn_dependency_task<T, H>(
        &self,
        dep: impl Dependency<T> + Send + 'static,
        handle: impl HandleDepResult<Result = T>,
    ) {
        let join_handle = DependencyTask { dep, handle }.run();
        self.register(join_handle).await;
    }
    pub async fn join_all(self) -> Vec<()> {
        #[cfg(async_executor_impl = "async-std")]
        let ret = join_all(self.task_handles.into_inner()).await;
        #[cfg(async_executor_impl = "tokio")]
        let ret = try_join_all(self.task_handles.into_inner()).await.unwrap();
        ret
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_broadcast::broadcast;
    #[cfg(async_executor_impl = "async-std")]
    use async_std::task::sleep;
    use std::{collections::HashSet, time::Duration};
    #[cfg(async_executor_impl = "tokio")]
    use tokio::time::sleep;

    #[derive(Default)]
    pub struct DummyHandle {
        val: usize,
        seen: HashSet<usize>,
    }

    impl TaskState for DummyHandle {
        type Event = usize;
        type Result = ();
        async fn handle_event(event: usize, task: &mut Task<Self>) -> Option<()> {
            sleep(Duration::from_millis(10)).await;
            let state = task.state_mut();
            state.seen.insert(event);
            if event > state.val {
                state.val = event;
                if state.val >= 100 {
                    panic!("Test should shutdown before getting an event for 100")
                }
                task.send(event + 1).await.unwrap();
            }
            None
        }
        fn should_shutdown(event: &usize) -> bool {
            *event >= 98
        }
        async fn shutdown(&mut self) {
            for i in 1..98 {
                assert!(self.seen.contains(&i));
            }
        }
    }

    impl TestTaskState for DummyHandle {
        type Message = String;
        type Result = ();
        type State = Self;

        async fn handle_message(
            message: Self::Message,
            _id: usize,
            _task: &mut TestTask<Self::State, Self>,
        ) -> Option<()> {
            println!("got message {}", message);
            if message == *"done".to_string() {
                return Some(());
            }
            None
        }
    }
    #[cfg_attr(
        async_executor_impl = "tokio",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    async fn it_works() {
        let reg = Arc::new(TaskRegistry::default());
        let (tx, rx) = broadcast(10);
        let task1 = Task::<DummyHandle> {
            event_sender: tx.clone(),
            event_receiver: rx.clone(),
            registry: reg.clone(),
            state: Default::default(),
        };
        tx.broadcast(1).await.unwrap();
        let task2 = Task::<DummyHandle> {
            event_sender: tx.clone(),
            event_receiver: rx,
            registry: reg,
            state: Default::default(),
        };
        let handle = task2.run();
        let _res = task1.run().await;
        let _ = handle.await;
    }

    #[cfg_attr(
        async_executor_impl = "tokio",
        tokio::test(flavor = "multi_thread", worker_threads = 10)
    )]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    #[allow(clippy::should_panic_without_expect)]
    #[should_panic]
    async fn test_works() {
        let reg = Arc::new(TaskRegistry::default());
        let (tx, rx) = broadcast(10);
        let (msg_tx, msg_rx) = broadcast(10);
        let task1 = Task::<DummyHandle> {
            event_sender: tx.clone(),
            event_receiver: rx.clone(),
            registry: reg.clone(),
            state: Default::default(),
        };
        tx.broadcast(1).await.unwrap();
        let task2 = Task::<DummyHandle> {
            event_sender: tx.clone(),
            event_receiver: rx,
            registry: reg,
            state: Default::default(),
        };
        let test1 = TestTask::<_, DummyHandle> {
            task: task1,
            message_receivers: vec![msg_rx.clone()],
        };
        let test2 = TestTask::<_, DummyHandle> {
            task: task2,
            message_receivers: vec![msg_rx.clone()],
        };

        let handle = test1.run();
        let handle2 = test2.run();
        sleep(Duration::from_millis(30)).await;
        msg_tx.broadcast("done".into()).await.unwrap();
        #[cfg(async_executor_impl = "tokio")]
        {
            handle.await.unwrap();
            handle2.await.unwrap();
        }
        #[cfg(async_executor_impl = "async-std")]
        {
            handle.await;
            handle2.await;
        }
    }
}
