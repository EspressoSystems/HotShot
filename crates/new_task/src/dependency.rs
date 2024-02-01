use async_broadcast::Receiver;
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use futures::FutureExt;
use std::future::Future;

pub trait Dependency<T> {
    fn completed(self) -> impl Future<Output = T> + Send;
}

trait CombineDependencies<T: Clone + Send + Sync + 'static>:
    Sized + Dependency<T> + Send + 'static
{
    fn or<D: Dependency<T> + Send + 'static>(self, dep: D) -> OrDependency<T> {
        let mut or = OrDependency::from_deps(vec![self]);
        or.add_dep(dep);
        or
    }
    fn and<D: Dependency<T> + Send + 'static>(self, dep: D) -> AndDependency<T> {
        let mut and = AndDependency::from_deps(vec![self]);
        and.add_dep(dep);
        and
    }
}

pub struct AndDependency<T> {
    deps: Vec<BoxFuture<'static, T>>,
}
impl<T: Clone + Send + Sync> Dependency<Vec<T>> for AndDependency<T> {
    async fn completed(self) -> Vec<T> {
        let futures = FuturesUnordered::from_iter(self.deps);
        futures.collect().await
    }
}

impl<T: Clone + Send + Sync + 'static> AndDependency<T> {
    pub fn from_deps(deps: Vec<impl Dependency<T> + Send + 'static>) -> Self {
        let mut pinned = vec![];
        for dep in deps {
            pinned.push(dep.completed().boxed())
        }
        Self { deps: pinned }
    }
    pub fn add_dep(&mut self, dep: impl Dependency<T> + Send + 'static) {
        self.deps.push(dep.completed().boxed());
    }
    pub fn add_deps(&mut self, deps: AndDependency<T>) {
        for dep in deps.deps {
            self.deps.push(dep);
        }
    }
}

pub struct OrDependency<T> {
    deps: Vec<BoxFuture<'static, T>>,
}
impl<T: Clone + Send + Sync> Dependency<T> for OrDependency<T> {
    async fn completed(self) -> T {
        let mut futures = FuturesUnordered::from_iter(self.deps);
        loop {
            if let Some(val) = futures.next().await {
                break val;
            }
        }
    }
}

impl<T: Clone + Send + Sync + 'static> OrDependency<T> {
    pub fn from_deps(deps: Vec<impl Dependency<T> + Send + 'static>) -> Self {
        let mut pinned = vec![];
        for dep in deps {
            pinned.push(dep.completed().boxed())
        }
        Self { deps: pinned }
    }
    pub fn add_dep(&mut self, dep: impl Dependency<T> + Send + 'static) {
        self.deps.push(dep.completed().boxed());
    }
}

pub struct EventDependency<T: Clone + Send + Sync> {
    pub(crate) event_rx: Receiver<T>,
    pub(crate) match_fn: Box<dyn Fn(&T) -> bool + Send>,
}

impl<T: Clone + Send + Sync + 'static> EventDependency<T> {
    pub fn new(receiver: Receiver<T>, match_fn: Box<dyn Fn(&T) -> bool + Send>) -> Self {
        Self {
            event_rx: receiver,
            match_fn: Box::new(match_fn),
        }
    }
}

impl<T: Clone + Send + Sync + 'static> Dependency<T> for EventDependency<T> {
    async fn completed(mut self) -> T {
        loop {
            let next = self.event_rx.recv().await.unwrap();
            if (self.match_fn)(&next) {
                return next;
            }
        }
    }
}

// Impl Combine for all the basic dependency types
impl<T: Clone + Send + Sync + 'static, D> CombineDependencies<T> for D where
    D: Dependency<T> + Send + 'static
{
}

#[cfg(test)]
mod tests {
    use crate::dependency::CombineDependencies;

    use super::{AndDependency, Dependency, EventDependency, OrDependency};
    use async_broadcast::{broadcast, Receiver};

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

        let mut deps = vec![];
        for i in 0..5 {
            tx.broadcast(i).await.unwrap();
            deps.push(eq_dep(rx.clone(), 5))
        }

        let and = AndDependency::from_deps(deps);
        tx.broadcast(5).await.unwrap();
        let result = and.completed().await;
        assert_eq!(result, vec![5; 5]);
    }
    #[cfg_attr(
        async_executor_impl = "tokio",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    async fn or_dep() {
        let (tx, rx) = broadcast(10);

        tx.broadcast(5).await.unwrap();
        let mut deps = vec![];
        for _ in 0..5 {
            deps.push(eq_dep(rx.clone(), 5))
        }
        let or = OrDependency::from_deps(deps);
        let result = or.completed().await;
        assert_eq!(result, 5);
    }

    #[cfg_attr(
        async_executor_impl = "tokio",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    async fn and_or_dep() {
        let (tx, rx) = broadcast(10);

        tx.broadcast(1).await.unwrap();
        tx.broadcast(2).await.unwrap();
        tx.broadcast(3).await.unwrap();
        tx.broadcast(5).await.unwrap();
        tx.broadcast(6).await.unwrap();

        let or1 = OrDependency::from_deps([eq_dep(rx.clone(), 4), eq_dep(rx.clone(), 6)].into());
        let or2 = OrDependency::from_deps([eq_dep(rx.clone(), 4), eq_dep(rx.clone(), 5)].into());
        let and = AndDependency::from_deps([or1, or2].into());
        let result = and.completed().await;
        assert_eq!(result, vec![6, 5]);
    }

    #[cfg_attr(
        async_executor_impl = "tokio",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    async fn or_and_dep() {
        let (tx, rx) = broadcast(10);

        tx.broadcast(1).await.unwrap();
        tx.broadcast(2).await.unwrap();
        tx.broadcast(3).await.unwrap();
        tx.broadcast(4).await.unwrap();
        tx.broadcast(5).await.unwrap();

        let and1 = eq_dep(rx.clone(), 4).and(eq_dep(rx.clone(), 6));
        let and2 = eq_dep(rx.clone(), 4).and(eq_dep(rx.clone(), 5));
        let or = and1.or(and2);
        let result = or.completed().await;
        assert_eq!(result, vec![4, 5]);
    }

    #[cfg_attr(
        async_executor_impl = "tokio",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    async fn many_and_dep() {
        let (tx, rx) = broadcast(10);

        tx.broadcast(1).await.unwrap();
        tx.broadcast(2).await.unwrap();
        tx.broadcast(3).await.unwrap();
        tx.broadcast(4).await.unwrap();
        tx.broadcast(5).await.unwrap();
        tx.broadcast(6).await.unwrap();

        let mut and1 = eq_dep(rx.clone(), 4).and(eq_dep(rx.clone(), 6));
        let and2 = eq_dep(rx.clone(), 4).and(eq_dep(rx.clone(), 5));
        and1.add_deps(and2);
        let result = and1.completed().await;
        assert_eq!(result, vec![4, 6, 4, 5]);
    }
}
