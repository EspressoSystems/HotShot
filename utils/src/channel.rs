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
