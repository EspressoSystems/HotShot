use futures::Future;
use hotshot_task::event_stream::SendableStream;
use hotshot_task::{event_stream::ChannelStream, task_impls::HSTWithEvent, task::PassType};
use std::marker::PhantomData;
use std::time::Duration;
use futures::{Stream, stream::Unfold};
use futures::stream::unfold;
use async_compatibility_layer::art::async_sleep;
use rand::{prelude::Distribution, thread_rng};


///  builder
pub mod test_builder;

/// launcher
pub mod test_launcher;

/// runner
pub mod test_runner;

/// task that's consuming events and asserting safety
pub mod safety_task;

// TODO control task for spinning up and down nodes
// will need to share with other tasks

/// task that's submitting transactions to the stream
pub mod txn_task;

/// task that decides when things are complete
pub mod completion_task;

/// context used by the safety task
pub mod node_ctx;

// TODO node changer (spin up and down)

#[derive(Clone, Debug)]
pub enum GlobalTestEvent {
    ShutDown
}

impl PassType for GlobalTestEvent {}

pub enum ShutDownReason {
    SafetyViolation,
    SuccessfullyCompleted,
}

pub type TestTask<ERR, STATE> = HSTWithEvent<ERR, GlobalTestEvent, ChannelStream<GlobalTestEvent>, STATE>;

// pub type empty_unfold = Unfold<(), impl Fn(()) -> impl Future<Output = Option<((), ())>>, impl Future<Output = Option<((), ())>>>;

// create a timer that periodically fires
// disguses it as a stream, sneakily
// pub fn create_timer_stream(secs: u64, nanos: u32) -> impl Stream<Item = ()>{
//     let tmp : Unfold<(), dyn FnMut() -> (), u32> = unfold((), move |_| async move {
//         Some((async_sleep(Duration::new(secs, nanos)).await, ()))
//     });
//     tmp
// }
//
// // Unfold<(), impl Fn(()) -> impl Future<Output = Option<((), ())>>, impl Future<Output = Option<((), ())>>>
//
// // Unfold<(DIST, ThreadRng), impl Fn((DIST, ThreadRng)) -> impl Future<Output = Option<((), (DIST, ThreadRng))>>, impl Future<Output = Option<((), (DIST, ThreadRng))>>>
//
// /// create a timer that periodically fires as a function of a distribution
// /// disguses it as a stream, sneaky sneaky
// pub fn create_timer_sample_stream<DIST: Distribution<u64>>(dist: DIST)  -> impl Stream<Item = ()> {
//     let rng = thread_rng();
//     let tmp = unfold((dist, rng), move |(dist, mut rng)| async move {
//         let sample = dist.sample(&mut rng);
//         Some((async_sleep(Duration::from_nanos(sample)).await, (dist, rng)))
//     });
//     tmp
// }
//
