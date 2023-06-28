//! Abstractions meant for usage with long running consensus tasks
//! and testing harness
#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::panic
)]

use Poll::{Pending, Ready};

use event_stream::SendableStream;
// The spawner of the task should be able to fire and forget the task if it makes sense.
use futures::{stream::Fuse, Stream, StreamExt, Future, future::BoxFuture};
use nll::nll_todo::nll_todo;
use std::{
    pin::Pin,
    task::{Context, Poll}, marker::PhantomData, sync::Arc,
};
// NOTE use pin_project here because we're already bring in procedural macros elsewhere
// so there is no reason to use pin_project_lite
use pin_project::pin_project;

/// Astractions over the state of a task and a stream
/// interface for task changes. Allows in the happy path
/// for lockless manipulation of tasks
/// and in the sad case, only the use of a `std::sync::mutex`
pub mod task_state;

/// the global registry storing the status of all tasks
/// as well as the abiliity to terminate them
pub mod global_registry;

/// mpmc streamable to all subscribed tasks
pub mod event_stream;

/// The `HotShot` Task. The main point of this library. Uses all other abstractions
/// to create an abstraction over tasks
pub mod task;

/// The hotshot task launcher. Useful for constructing tasks
pub mod task_launcher;

/// the task implementations with different features
pub mod task_impls;

// NOTE: yoinked /from async-std
// except this is executor agnostic (doesn't rely on async-std streamext/fuse)
// NOTE: usage of this is for combining streams into one main stream
// for usage with `MessageStream`
// TODO move this to async-compatibility-layer
#[pin_project]
/// Stream returned by the [`merge`](super::StreamExt::merge) method.
pub struct Merge<T, U> {
    /// first stream to merge
    #[pin]
    a: Fuse<T>,
    /// second stream to merge
    #[pin]
    b: Fuse<U>,
    /// When `true`, poll `a` first, otherwise, `poll` b`.
    a_first: bool,
}

impl<T, U> Merge<T, U> {
    /// create a new Merged stream
    pub fn new(a: T, b: U) -> Merge<T, U>
    where
        T: Stream,
        U: Stream,
    {
        Merge {
            a: a.fuse(),
            b: b.fuse(),
            a_first: true,
        }
    }
}

impl<T, U> Stream for Merge<T, U>
where
    T: Stream,
    U: Stream<Item = T::Item>,
{
    type Item = T::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T::Item>> {
        let me = self.project();
        let a_first = *me.a_first;

        // Toggle the flag
        *me.a_first = !a_first;

        if a_first {
            poll_next(me.a, me.b, cx)
        } else {
            poll_next(me.b, me.a, cx)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (a_lower, a_upper) = self.a.size_hint();
        let (b_lower, b_upper) = self.b.size_hint();

        let upper = match (a_upper, b_upper) {
            (Some(a_upper), Some(b_upper)) => Some(a_upper + b_upper),
            _ => None,
        };

        (a_lower + b_lower, upper)
    }
}

/// poll the next item in the merged stream
fn poll_next<T, U>(
    first: Pin<&mut T>,
    second: Pin<&mut U>,
    cx: &mut Context<'_>,
) -> Poll<Option<T::Item>>
where
    T: Stream,
    U: Stream<Item = T::Item>,
{
    let mut done = true;

    match first.poll_next(cx) {
        Ready(Some(val)) => return Ready(Some(val)),
        Ready(None) => {}
        Pending => done = false,
    }

    match second.poll_next(cx) {
        Ready(Some(val)) => return Ready(Some(val)),
        Ready(None) => {}
        Pending => done = false,
    }

    if done {
        Ready(None)
    } else {
        Pending
    }
}

/// gotta make the futures sync
pub type BoxSyncFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + Sync + 'a>>;

#[pin_project(project = ProjectedStreamableThing)]
pub struct GeneratedStream<O>  {
    // todo maybe type wrapper is in order
    generator: Arc<dyn Fn() -> BoxSyncFuture<'static, O> + Sync + Send>,
    in_progress_fut: Option<BoxSyncFuture<'static, O>>,
}

impl<O> GeneratedStream<O> {
    pub fn new(generator: Arc<dyn Fn() -> BoxSyncFuture<'static, O> + Sync + Send>,) -> Self{
        GeneratedStream { generator, in_progress_fut: None }
    }
}

impl<O> Stream for GeneratedStream<O> {
    type Item = O;

    fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        let projection = self.project();
        match projection.in_progress_fut {
            Some(fut) => {
                // NOTE: this is entirely safe.
                // We will ONLY poll if we've been awakened.
                // otherwise, we won't poll.
                match fut.as_mut().poll(cx) {
                    Ready(val) => {
                        *projection.in_progress_fut = None;
                        return Poll::Ready(Some(val));
                    },
                    Pending => {
                        return Poll::Pending;
                    },
                }
            },
            None => {
                let mut fut = (*projection.generator)();
                match fut.as_mut().poll(cx) {
                    Ready(val) => {
                        *projection.in_progress_fut = None;
                        return Poll::Ready(Some(val));
                    },
                    Pending => {
                        *projection.in_progress_fut = Some(fut);
                        return Poll::Pending;
                    },
                }
            },
        }
    }
}

/// yoinked from futures crate
pub fn assert_future<T, F>(future: F) -> F
where
    F: Future<Output = T>,
{
    future
}

/// yoinked from futures crate, adds sync bound that we need
fn boxed_sync<'a, F>(fut: F) -> BoxSyncFuture<'a, F::Output>
where
F: Future + Sized + Send + 'a + Sync,
{
    assert_future::<F::Output, _>(Box::pin(fut))
}

impl<O : Sync + Send + 'static> SendableStream for GeneratedStream<O> {

}

#[cfg(test)]
pub mod test {
    use crate::*;

    #[cfg(test)]
    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    async fn test_stream_basic() {
        let mut stream = GeneratedStream::<u32>{
            generator: Arc::new(move || {
               let closure = async move {
                   5

               };
               boxed_sync(closure)
            }
            ),
            in_progress_fut: None
        };
        assert!(stream.next().await == Some(5));
        assert!(stream.next().await == Some(5));
        assert!(stream.next().await == Some(5));
        assert!(stream.next().await == Some(5));
    }

    #[cfg(test)]
    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    async fn test_stream_fancy() {
        use std::sync::atomic::Ordering;
        use async_compatibility_layer::art::async_sleep;
        use std::time::Duration;

        let value = Arc::<std::sync::atomic::AtomicU8>::default();
        let mut stream = GeneratedStream::<u32>{
            generator: Arc::new(move || {
               let value = value.clone();
               let closure = async move {
                   let actual_value = value.load(Ordering::Relaxed);
                   value.store(actual_value + 1, Ordering::Relaxed);
                   async_sleep(Duration::new(0, 500)).await;
                   actual_value as u32
               };
               boxed_sync(closure)
            }
            ),
            in_progress_fut: None
        };
        assert!(stream.next().await == Some(0));
        assert!(stream.next().await == Some(1));
        assert!(stream.next().await == Some(2));
        assert!(stream.next().await == Some(3));
    }
}

