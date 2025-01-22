use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use derive_more::derive::Deref;
use tokio::task::{JoinError, JoinHandle};

/// A wrapper for a `JoinHandle` that will automatically abort the task if dropped
#[derive(Debug, Deref)]
pub struct AbortOnDropHandle<T>(pub JoinHandle<T>);

impl<T> Drop for AbortOnDropHandle<T> {
    fn drop(&mut self) {
        // Abort the task
        self.0.abort();
    }
}

/// Delegate the future to the inner join handle
impl<T> Future for AbortOnDropHandle<T> {
    type Output = Result<T, JoinError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}
