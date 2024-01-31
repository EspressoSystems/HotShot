#[cfg(async_executor_impl = "async-std")]
use async_std::task::{spawn, JoinHandle};
#[cfg(async_executor_impl = "tokio")]
use tokio::task::{spawn, JoinHandle};

use futures::Future;

use crate::dependency::Dependency;

pub trait HandleDepResult: Send + Sized + Sync + 'static {
    type Result: Send + Sync + 'static;

    /// Called once when the Dependency completes handles the results
    fn handle_dep_result(self, res: Self::Result) -> impl Future<Output = ()> + Send;
}

pub struct DependencyTask<D: Dependency<H::Result> + Send, H: HandleDepResult + Send> {
    pub(crate) dep: D,
    pub(crate) handle: H,
}

impl<D: Dependency<H::Result> + Send, H: HandleDepResult + Send> DependencyTask<D, H> {
    pub fn new(dep: D, handle: H) -> Self {
        Self { dep, handle }
    }
}

impl<D: Dependency<H::Result> + Send + 'static, H: HandleDepResult> DependencyTask<D, H> {
    pub fn run(self) -> JoinHandle<()>
    where
        Self: Sized,
    {
        spawn(async move {
            self.handle
                .handle_dep_result(self.dep.completed().await)
                .await;
        })
    }
}

#[cfg(test)]
mod test {

    use std::time::Duration;

    use async_broadcast::{broadcast, Receiver, Sender};
    use futures::{stream::FuturesOrdered, StreamExt};

    #[cfg(async_executor_impl = "async-std")]
    use async_std::task::sleep;
    #[cfg(async_executor_impl = "tokio")]
    use tokio::time::sleep;

    use super::*;
    use crate::dependency::*;

    #[derive(Clone, PartialEq, Eq, Debug)]
    enum TaskResult {
        Success(usize),
        // Failure,
    }

    struct DummyHandle {
        sender: Sender<TaskResult>,
    }
    impl HandleDepResult for DummyHandle {
        type Result = usize;
        async fn handle_dep_result(self, res: usize) {
            self.sender
                .broadcast(TaskResult::Success(res))
                .await
                .unwrap();
        }
    }

    fn eq_dep(rx: Receiver<usize>, val: usize) -> EventDependency<usize> {
        EventDependency {
            event_rx: rx,
            match_fn: Box::new(move |v| *v == val),
        }
    }

    #[cfg_attr(
        async_executor_impl = "tokio",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    async fn it_works() {
        let (tx, rx) = broadcast(10);
        let (res_tx, mut res_rx) = broadcast(10);
        let dep = eq_dep(rx, 2);
        let handle = DummyHandle { sender: res_tx };
        let join_handle = DependencyTask { dep, handle }.run();
        tx.broadcast(2).await.unwrap();
        assert_eq!(res_rx.recv().await.unwrap(), TaskResult::Success(2));
        let _ = join_handle.await;
    }

    #[cfg_attr(
        async_executor_impl = "tokio",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    async fn many_works() {
        let (tx, rx) = broadcast(20);
        let (res_tx, mut res_rx) = broadcast(20);

        let mut handles = vec![];
        for i in 0..10 {
            let dep = eq_dep(rx.clone(), i);
            let handle = DummyHandle {
                sender: res_tx.clone(),
            };
            handles.push(DependencyTask { dep, handle }.run());
        }
        let tx2 = tx.clone();
        spawn(async move {
            for i in 0..10 {
                tx.broadcast(i).await.unwrap();
                sleep(Duration::from_millis(10)).await;
            }
        });
        for i in 0..10 {
            assert_eq!(res_rx.recv().await.unwrap(), TaskResult::Success(i));
        }
        tx2.broadcast(100).await.unwrap();
        FuturesOrdered::from_iter(handles).collect::<Vec<_>>().await;
    }
}
