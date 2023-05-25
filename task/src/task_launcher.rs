use futures::{Future, future::{BoxFuture, join_all}};
use nll::nll_todo::nll_todo;

use crate::{task::{TaskErr, HotShotTaskCompleted}, global_registry::{GlobalRegistry, HotShotTaskId}};

/// runner for tasks
pub struct TaskRunner<
    const N: usize,
> {
    /// this is the most noncommital thing ever
    tasks: [(HotShotTaskId, String, BoxFuture<'static, HotShotTaskCompleted<dyn TaskErr>>); N],
    /// global registry
    registry: GlobalRegistry,
}

impl<const N: usize> TaskRunner<N> {
    pub async fn launch(self) -> Vec<(String, HotShotTaskCompleted<dyn TaskErr>)> {
        let names = self.tasks.iter().map(|(_id, name, _)| name.clone()).collect::<Vec<_>>();
        let result = join_all(self.tasks.into_iter().map(|(_, _, task)| task)).await;

        names.into_iter().zip(result).collect::<Vec<_>>()
    }
}

