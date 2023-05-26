use std::pin::Pin;

use futures::{future::{BoxFuture, join_all}, FutureExt};
use arrayvec::{ArrayVec, CapacityError};

use crate::{task::{TaskErr, HotShotTaskCompleted, HotShotTaskTypes}, global_registry::{GlobalRegistry, HotShotTaskId}, task_impls::TaskBuilder};

/// runner for tasks
/// `N` specifies the number of tasks to ensure that the user
/// doesn't forget how many tasks they wished to add.
pub struct TaskRunner<
    const N: usize,
> {
    /// this is the most noncommital thing ever
    /// as we're allowing entirely different generics for each task.
    tasks: ArrayVec<(HotShotTaskId, String, BoxFuture<'static, HotShotTaskCompleted<dyn TaskErr>>), N>,
    /// global registry
    pub registry: GlobalRegistry,
}

impl<const N: usize> TaskRunner<N> {
    /// create new runner
    pub fn new() -> Self {
        Self {
            tasks: ArrayVec::new(),
            registry: GlobalRegistry::new(),
        }
    }


    /// to support builder pattern
    pub fn add_task<HSTT: HotShotTaskTypes<Error = (dyn TaskErr + 'static)>>(&mut self, id: HotShotTaskId, name: String, builder: TaskBuilder<HSTT>) ->
        Result<(), CapacityError<(
            usize,
            String,
            BoxFuture<'static, HotShotTaskCompleted<(dyn TaskErr + 'static)>>
        )>> {
        self.tasks.try_push((id, name, HSTT::build(builder).launch().boxed()))
    }


    /// returns a `Vec` because type isn't known
    pub async fn launch(self) -> Vec<(String, HotShotTaskCompleted<dyn TaskErr>)> {
        let names = self.tasks.iter().map(|(_id, name, _)| name.clone()).collect::<Vec<_>>();
        let result = join_all(self.tasks.into_iter().map(|(_, _, task)| task)).await;

        names.into_iter().zip(result).collect::<Vec<_>>()
    }
}

