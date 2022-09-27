//! Abstraction over channels. This will expose the following types:
//!
//! - [`unbounded`] returning an [`UnboundedSender`] and [`UnboundedReceiver`]
//! - [`bounded`] returning an [`BoundedSender`] and [`BoundedReceiver`]
//! - [`oneshot`] returning an [`OneShotSender`] and [`OneShotReceiver`]
//! - several error types
//!
//! Which channel is selected depends on these feature flags:
//! - `channel-tokio` enables [tokio](https://docs.rs/tokio)
//! - `channel-async-std` enables [async-std](https://docs.rs/async-std)
//! - `channel-flume` enables [flume](https://docs.rs/flume)
//!
//! Note that exactly 1 of these features must be enabled. If 0 or 2+ are selected, you'll get compiler errors.
//!
//! Some of these implementations may not exist under the crate selected, in those cases they will be shimmed to another channel. e.g. `oneshot` might be implemented as `bounded(1)`.

// VKO: If you see this in the PR I have forgotten to remove it
#![allow(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::missing_errors_doc
)]

mod bounded;
mod oneshot;
mod unbounded;

pub use bounded::{bounded, Receiver, RecvError, SendError, Sender, TryRecvError};
pub use oneshot::{oneshot, OneShotReceiver, OneShotRecvError, OneShotSender, OneShotTryRecvError};
pub use unbounded::{
    unbounded, UnboundedReceiver, UnboundedRecvError, UnboundedSendError, UnboundedSender,
    UnboundedTryRecvError,
};

// use cfg_if::cfg_if;
// use futures::{future::FusedFuture, FutureExt};
// pub use inner::{OneShotRecvError, RecvError, SendError, TryRecvError, TrySendError};

// mod inner {
//     cfg_if::cfg_if! {
//         if #[cfg(feature = "channel-tokio")] {
//             pub use tokio::sync::mpsc::error::{SendError, TrySendError, TryRecvError};

//             pub use tokio::sync::mpsc::{
//                 channel as bounded,
//                 Sender as BoundedSender,
//                 Receiver as BoundedReceiver,
//             };

//             pub use tokio::sync::mpsc::{
//                 unbounded_channel as unbounded,
//                 UnboundedSender,
//                 UnboundedReceiver,
//             };

//             pub use tokio::sync::oneshot::{
//                 channel as oneshot,
//                 Sender as OneShotSender,
//                 Receiver as OneShotReceiver,
//                 error::RecvError as OneShotRecvError,
//             };

//             #[derive(Debug, PartialEq, Eq)]
//             pub struct RecvError;

//             impl std::fmt::Display for RecvError {
//                 fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//                     write!(fmt, stringify!(RecvError))
//                 }
//             }

//             impl std::error::Error for RecvError {}

//             /// Turn a `TryRecvError` into a `RecvError` if it's not `Empty`
//             pub fn try_recv_error_to_recv_error(e: TryRecvError) -> Option<RecvError> {
//                 match e {
//                     TryRecvError::Empty => None,
//                     TryRecvError::Disconnected => Some(RecvError),
//                 }
//             }
//         } else if #[cfg(feature = "channel-async-std")] {
//             pub use async_std::channel::{TryRecvError, TrySendError, RecvError, SendError, RecvError as OneShotRecvError};

//             pub use async_std::channel::{
//                 bounded,
//                 Sender as BoundedSender,
//                 Receiver as BoundedReceiver,
//             };

//             pub use async_std::channel::{
//                 unbounded,
//                 Sender as UnboundedSender,
//                 Receiver as UnboundedReceiver,
//             };

//             /// Create a oneshot channel, which will allow sending a message exactly once
//             pub fn oneshot<T>() -> (OneShotSender<T>, OneShotReceiver<T>) {
//                 let (sender, receiver) = bounded(1);
//                 (OneShotSender(sender), receiver)
//             }
//             pub struct OneShotSender<T>(BoundedSender<T>);
//             impl<T> OneShotSender<T> {
//                 pub fn send(self, msg: T) -> Result<(), T> {
//                     self.0.try_send(msg).map_err(|e| e.into_inner())
//                 }
//             }
//             pub use BoundedReceiver as OneShotReceiver;

//             /// Turn a `TryRecvError` into a `RecvError` if it's not `Empty`
//             pub fn try_recv_error_to_recv_error(e: TryRecvError) -> Option<RecvError> {
//                 match e {
//                     TryRecvError::Empty => None,
//                     TryRecvError::Closed => Some(RecvError),
//                 }
//             }
//         } else if #[cfg(feature = "channel-flume")] {
//             pub use flume::{TryRecvError, RecvError, TrySendError, SendError};

//             pub use flume::{
//                 bounded,
//                 Sender as BoundedSender,
//                 Receiver as BoundedReceiver,
//             };
//             pub use flume::{
//                 unbounded,
//                 Sender as UnboundedSender,
//                 Receiver as UnboundedReceiver,
//             };

//             /// Create a oneshot channel, which will allow sending a message exactly once
//             pub fn oneshot<T>() -> (OneShotSender<T>, OneShotReceiver<T>) {
//                 let (sender, receiver) = bounded(1);
//                 (OneShotSender(sender), OneShotReceiver(receiver))
//             }
//             pub struct OneShotSender<T>(BoundedSender<T>);
//             impl<T> OneShotSender<T> {
//                 pub fn send(self, msg: T) -> Result<(), T> {
//                     self.0.try_send(msg).map_err(TrySendError::into_inner)
//                 }
//             }
//             pub struct OneShotReceiver<T>(BoundedReceiver<T>);
//             impl<T> OneShotReceiver<T> {
//                 pub async fn recv(&self) -> Result<T, RecvError> {
//                     self.0.recv_async().await
//                 }
//             }

//             /// Turn a `TryRecvError` into a `RecvError` if it's not `Empty`
//             pub fn try_recv_error_to_recv_error(e: TryRecvError) -> Option<RecvError> {
//                 match e {
//                     TryRecvError::Empty => None,
//                     TryRecvError::Disconnected => Some(RecvError::Disconnected),
//                 }
//             }
//         }
//     }
// }

// // Clone impl
// impl<T> Clone for UnboundedSender<T> {
//     fn clone(&self) -> Self {
//         Self(self.0.clone())
//     }
// }
// // impl<T> Clone for UnboundedReceiver<T> {
// //     fn clone(&self) -> Self {
// //         Self(self.0.clone())
// //     }
// // }
// impl<T> Clone for BoundedSender<T> {
//     fn clone(&self) -> Self {
//         Self(self.0.clone())
//     }
// }
// // impl<T> Clone for BoundedReceiver<T> {
// //     fn clone(&self) -> Self {
// //         Self(self.0.clone())
// //     }
// // }

// // Debug impl
// impl<T> std::fmt::Debug for UnboundedSender<T> {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.debug_struct("UnboundedSender").finish()
//     }
// }
// impl<T> std::fmt::Debug for UnboundedReceiver<T> {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.debug_struct("UnboundedReceiver").finish()
//     }
// }
// impl<T> std::fmt::Debug for BoundedSender<T> {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.debug_struct("BoundedSender").finish()
//     }
// }
// impl<T> std::fmt::Debug for BoundedReceiver<T> {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.debug_struct("BoundedReceiver").finish()
//     }
// }

// impl<T> std::fmt::Debug for OneShotSender<T> {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.debug_struct("OneShotSender").finish()
//     }
// }
// impl<T> std::fmt::Debug for OneShotReceiver<T> {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.debug_struct("OneShotReceiver").finish()
//     }
// }
