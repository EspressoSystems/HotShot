use async_lock::RwLock;
use either::Either;
use std::ops::Deref;
use std::sync::Arc;

use crate::task_state::{HotShotTaskState, HotShotTaskStatus};

/// function to shut down gobal registry
pub struct ShutdownFn(Box<dyn Fn()>);

// TODO this would be cleaner as `run()`
impl Deref for ShutdownFn {
    type Target = dyn Fn();

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
    state_list: Arc<RwLock<Vec<(HotShotTaskState, String)>>>,
    /// possibly stale read version of state
    /// NOTE: must include entire state in order to
    /// support both incrementing and reading
    /// writing to the status should gracefully shut down the task
    state_cache: Vec<(HotShotTaskState, String)>,
}

/// function to modify state
struct Modifier(Box<dyn Fn(&HotShotTaskState) -> Either<HotShotTaskStatus, bool>>);

impl GlobalRegistry {
    /// create new registry
    pub fn spawn_new() -> Self {
        Self {
            state_list: Arc::new(RwLock::new(vec![])),
            state_cache: vec![],
        }
    }

    /// register with the global registry
    /// return a function to the caller (task) that can be used to deregister
    /// returns a function to call to shut down the task
    /// and the unique identifier of the task
    pub async fn register(
        &mut self,
        name: &str,
        status: HotShotTaskState,
    ) -> (ShutdownFn, HotShotTaskId) {
        let mut list = self.state_list.write().await;
        let next_id = list.len();
        let new_entry = (status.clone(), name.to_string());
        let new_entry_dup = new_entry.0.clone();
        list.push(new_entry);

        for i in self.state_cache.len()..list.len() {
            self.state_cache.push(list[i].clone());
        }

        let shutdown_fn = ShutdownFn(Box::new(move || {
            new_entry_dup.set_state(HotShotTaskStatus::Completed);
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
    ) -> Either<HotShotTaskStatus, bool> {
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
            state.set_state(HotShotTaskStatus::Paused);
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
            state.set_state(HotShotTaskStatus::Running);
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
    /// since HotShotTaskStatus implements stream
    pub async fn get_task_state(&mut self, uid: HotShotTaskId) -> Option<HotShotTaskStatus> {
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
            state.set_state(HotShotTaskStatus::Completed);
            Either::Right(true)
        }));
        match self.operate_on_task(uid, modifier).await {
            Either::Left(_) => unreachable!(),
            Either::Right(b) => b,
        }
    }
}
