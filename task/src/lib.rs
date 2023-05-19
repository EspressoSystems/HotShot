#[allow(clippy::non_camel_case_types)]
use async_compatibility_layer::channel::{
    unbounded, Sender, UnboundedReceiver, UnboundedSender, UnboundedStream,
};
use async_lock::RwLock;
// Async tasks will be the building blocks for the run view refactor.
// An async task should be spannable by some trigger. That could be some other task completing or some event coming from the network.
//
// Task should be able to
// - publish messages to a shared event stream (e.g. a view sync task can publish a view change event)
// - register themselves with a shared task registry
// - consume events from the shared event stream. Every task must handle the shutdown event.
// - remove themselves from the registry on their competition

// The spawner of the task should be able to fire and forget the task if it makes sense.
use async_trait::async_trait;
use atomic_enum::atomic_enum;
use either::Either;
use event_stream::{EventStream, DummyStream};
use futures::{stream::Fuse, Future, Stream, StreamExt, future::{LocalBoxFuture}, FutureExt};
use global_registry::{ShutdownFn, HotShotTaskId, GlobalRegistry};
use serde::{Deserialize, Serialize};
use snafu::Snafu;
use task_state::{HotShotTaskStatus, HotShotTaskState};
use std::{
    collections::HashMap,
    marker::PhantomData,
    ops::Deref,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Waker},
};
// NOTE use pin_project here because we're already bring in procedural macros elsewhere
// so there is no reason to use pin_project_lite
use pin_project::pin_project;

pub mod task_state;

pub mod global_registry;

pub mod event_stream;

pub mod task;

// NOTE: yoinked /from async-std
// except this is executor agnostic (doesn't rely on async-std streamext/fuse)
// TODO move this to async-compatibility-layer
#[pin_project]
/// Stream returned by the [`merge`](super::StreamExt::merge) method.
pub struct Merge<T, U> {
    #[pin]
    a: Fuse<T>,
    #[pin]
    b: Fuse<U>,
    // When `true`, poll `a` first, otherwise, `poll` b`.
    a_first: bool,
}

impl<T, U> Merge<T, U> {
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

fn poll_next<T, U>(
    first: Pin<&mut T>,
    second: Pin<&mut U>,
    cx: &mut Context<'_>,
) -> Poll<Option<T::Item>>
where
    T: Stream,
    U: Stream<Item = T::Item>,
{
    use Poll::*;

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





// TODO revive this explicitly for tasks
// convenience launcher for tasks
// pub struct TaskLauncher<
//     const N: usize,
//     HST: HotS
// > {
//     tasks: [HotShotTask<EVENT, STATE, STREAM, MSG, MSG_STREAM>; N],
// }
