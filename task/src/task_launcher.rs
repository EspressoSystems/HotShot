use crate::{task::{TaskErr, TaskTrait}, global_registry::GlobalRegistry};

/// runner for tasks
pub struct TaskRunner<
    const N: usize,
> {
    /// this is the most noncommital thing ever
    tasks: [Box<dyn TaskTrait<dyn TaskErr>>; N],
    registry: GlobalRegistry,
}

