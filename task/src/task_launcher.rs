use crate::task::{TaskErr, TaskTrait};

// TODO revive this explicitly for tasks
// convenience launcher for tasks
pub struct TaskLauncher<
    const N: usize,
> {
    tasks: [Box<dyn TaskTrait<dyn TaskErr>>; N]
}
