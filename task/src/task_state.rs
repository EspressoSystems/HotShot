use std::{task::Waker, sync::{Mutex, Arc, atomic::Ordering}};

use atomic_enum::atomic_enum;
use futures::Stream;
use serde::{Serialize, Deserialize};

// Nit: wish this was for u8 but sadly no
#[atomic_enum]
#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum HotShotTaskStatus {
    /// the task hasn't started running
    NotStarted = 0,
    /// the task is running
    Running = 1,
    /// NOTE: not useful generally, but VERY useful for byzantine nodes
    /// and testing malfunctions
    /// we'll have a granular way to, from the registry, stop a task momentarily
    Paused = 2,
    /// the task completed
    Completed = 3,
}

/// The state of a task
/// `AtomicHotShotTaskStatus` + book keeping to notify btwn tasks
#[derive(Clone)]
pub struct HotShotTaskState {
    /// previous status
    prev: Arc<AtomicHotShotTaskStatus>,
    /// next status
    next: Arc<AtomicHotShotTaskStatus>,
    /// using `std::sync::mutex` here because it's faster than async's version
    wakers: Arc<Mutex<Vec<Waker>>>,
}

impl std::fmt::Debug for HotShotTaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HotShotTaskState")
            .field("status", &self.get_status())
            .finish()
    }
}

impl HotShotTaskState {
    /// create a new state
    pub fn new() -> Self {
        Self {
            prev: Arc::new(HotShotTaskStatus::NotStarted.into()),
            next: Arc::new(HotShotTaskStatus::NotStarted.into()),
            wakers: Arc::default(),
        }
    }

    pub fn from_status(state: Arc<AtomicHotShotTaskStatus>) -> Self {
        let prev_state = AtomicHotShotTaskStatus::new(state.load(Ordering::SeqCst));
        Self {
            prev: Arc::new(prev_state),
            next: state,
            wakers: Arc::default(),
        }
    }

    /// sets the state
    pub fn set_state(&self, state: HotShotTaskStatus) {
        self.next.swap(state, Ordering::SeqCst);
        // no panics, so can never be poisoned.
        let mut wakers = self.wakers.lock().unwrap();

        // drain the wakers
        for waker in wakers.drain(..) {
            waker.wake();
        }
    }
    /// gets a possibly stale version of the state
    pub fn get_status(&self) -> HotShotTaskStatus {
        self.next.load(Ordering::SeqCst)
    }
}

impl Stream for HotShotTaskState {
    type Item = HotShotTaskStatus;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let next = self.next.load(Ordering::SeqCst);
        let prev = self.prev.swap(next, Ordering::SeqCst);
        // a new value has been set
        if prev != next {
            std::task::Poll::Ready(Some(next))
        } else {
            // no panics, so impossible to be poisoned
            self.wakers.lock().unwrap().push(cx.waker().clone());

            // no value has been set, poll again later
            std::task::Poll::Pending
        }
    }
}

#[cfg(test)]
pub mod test {
    use async_compatibility_layer::art::{async_sleep, async_spawn};

    #[cfg(test)]
    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    async fn test_stream() {
        setup_logging();
        use async_compatibility_layer::logging::setup_logging;
        use futures::StreamExt;

        let mut task = crate::task_state::HotShotTaskState::new();

        let task_dup = task.clone();

        async_spawn(async move {
            async_sleep(std::time::Duration::from_secs(2)).await;
            task_dup.set_state(crate::HotShotTaskStatus::Running);
        });

        // spawn new task that sleeps then increments

        assert_eq!(
            task.next().await.unwrap(),
            crate::HotShotTaskStatus::Running
        );
    }
    // TODO test global registry using either global + lazy_static
    // or passing around global registry
}
