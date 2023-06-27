use async_lock::RwLock;
use either::Either;
use futures::{future::BoxFuture, FutureExt};
use std::ops::Deref;
use std::sync::Arc;

use crate::task_state::{TaskState, TaskStatus};

/// function to shut down gobal registry
#[derive(Clone)]
pub struct ShutdownFn(pub Arc<dyn Fn() -> BoxFuture<'static, ()> + Sync + Send>);

// TODO this might cleaner as `run()`
// but then this pattern should change everywhere
impl Deref for ShutdownFn {
    type Target = dyn Fn() -> BoxFuture<'static, ()> + Sync + Send;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

/// id of task. Usize instead of u64 because
/// used for primarily for indexing
pub type HotShotTaskId = usize;

/// the global registry provides a place to:
/// - inquire about the state of various tasks
/// - gracefully shut down tasks
#[derive(Debug, Clone)]
pub struct GlobalRegistry {
    /// up-to-date shared list of statuses
    /// only used if `state_cpy` is out of date
    /// or if appending
    state_list: Arc<RwLock<Vec<(TaskState, String)>>>,
    /// possibly stale read version of state
    /// NOTE: must include entire state in order to
    /// support both incrementing and reading.
    /// Writing to the status should gracefully shut down the task
    state_cache: Vec<(TaskState, String)>,
}

/// function to modify state
#[allow(clippy::type_complexity)]
struct Modifier(Box<dyn Fn(&TaskState) -> Either<TaskStatus, bool>>);

impl Default for GlobalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalRegistry {
    /// create new registry
    #[must_use]
    pub fn new() -> Self {
        Self {
            state_list: Arc::new(RwLock::new(vec![])),
            state_cache: vec![],
        }
    }

    /// register with the global registry
    /// return a function to the caller (task) that can be used to deregister
    /// returns a function to call to shut down the task
    /// and the unique identifier of the task
    pub async fn register(&mut self, name: &str, status: TaskState) -> (ShutdownFn, HotShotTaskId) {
        let mut list = self.state_list.write().await;
        let next_id = list.len();
        let new_entry = (status.clone(), name.to_string());
        let new_entry_dup = new_entry.0.clone();
        list.push(new_entry);

        for i in self.state_cache.len()..list.len() {
            self.state_cache.push(list[i].clone());
        }

        let shutdown_fn = ShutdownFn(Arc::new(move || {
            new_entry_dup.set_state(TaskStatus::Completed);
            async move {}.boxed()
        }));
        (shutdown_fn, next_id)
    }

    /// update the cache
    async fn update_cache(&mut self) {
        let list = self.state_list.read().await;
        for i in self.state_cache.len()..list.len() {
            self.state_cache.push(list[i].clone());
        }
    }

    /// internal function to run `modifier` on `uid`
    /// if it exists
    async fn operate_on_task(
        &mut self,
        uid: HotShotTaskId,
        modifier: Modifier,
    ) -> Either<TaskStatus, bool> {
        // the happy path
        if uid < self.state_cache.len() {
            modifier.0(&self.state_cache[uid].0)
        }
        // the sad path
        else {
            self.update_cache().await;
            if uid < self.state_cache.len() {
                modifier.0(&self.state_cache[uid].0)
            } else {
                Either::Right(false)
            }
        }
    }

    /// set `uid`'s state to paused
    /// returns true upon success and false if `uid` is not registered
    pub async fn pause_task(&mut self, uid: HotShotTaskId) -> bool {
        let modifier = Modifier(Box::new(|state| {
            state.set_state(TaskStatus::Paused);
            Either::Right(true)
        }));
        match self.operate_on_task(uid, modifier).await {
            Either::Left(_) => unreachable!(),
            Either::Right(b) => b,
        }
    }

    /// set `uid`'s state to running
    /// returns true upon success and false if `uid` is not registered
    pub async fn run_task(&mut self, uid: HotShotTaskId) -> bool {
        let modifier = Modifier(Box::new(|state| {
            state.set_state(TaskStatus::Running);
            Either::Right(true)
        }));
        match self.operate_on_task(uid, modifier).await {
            Either::Left(_) => unreachable!(),
            Either::Right(b) => b,
        }
    }

    /// if the `uid` is registered with the global registry
    /// return its task status
    /// this is a way to subscribe to state changes from the taskstatus
    /// since `HotShotTaskStatus` implements stream
    pub async fn get_task_state(&mut self, uid: HotShotTaskId) -> Option<TaskStatus> {
        let modifier = Modifier(Box::new(|state| Either::Left(state.get_status())));
        match self.operate_on_task(uid, modifier).await {
            Either::Left(state) => Some(state),
            Either::Right(false) => None,
            Either::Right(true) => unreachable!(),
        }
    }

    /// shut down a task from a different thread
    /// returns true if succeeded
    /// returns false if the task does not exist
    pub async fn shutdown_task(&mut self, uid: usize) -> bool {
        let modifier = Modifier(Box::new(|state| {
            state.set_state(TaskStatus::Completed);
            Either::Right(true)
        }));
        match self.operate_on_task(uid, modifier).await {
            Either::Left(_) => unreachable!(),
            Either::Right(b) => b,
        }
    }

    /// checks if all registered tasks have completed
    pub async fn is_shutdown(&mut self) -> bool {
        let task_list = self.state_list.read().await;
        for task in task_list.iter() {
            if task.0.get_status() != TaskStatus::Completed {
                return false;
            }
        }
        true
    }
}
