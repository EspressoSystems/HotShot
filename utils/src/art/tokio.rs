use tokio::net::{tcp::OwnedReadHalf, tcp::OwnedWriteHalf};
pub use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    main as async_main,
    net::TcpStream,
    task::spawn as async_spawn_local,
    test as async_test,
    time::{sleep as async_sleep, timeout as async_timeout},
};

#[cfg(feature = "profiling")]
/// spawn and log task id
pub fn async_spawn<T>(future: T) -> tokio::task::JoinHandle<T::Output>
where
    T: futures::Future + Send + 'static,
    T::Output: Send + 'static,
{
    let async_span = tracing::error_span!("Task Root", tsk_id = tracing::field::Empty);
    let join_handle = tokio::task::spawn(tracing::Instrument::instrument(
        async { future.await },
        async_span.clone(),
    ));
    async_span.record("tsk_id", tracing::field::display(join_handle.id()));
    join_handle
}
#[cfg(not(feature = "profiling"))]
pub use tokio::spawn as async_spawn;

/// Generates tokio runtime then provides `block_on` with `tokio` that matches `async_std::task::block_on`
/// # Panics
/// If we're already in a runtime
pub fn async_block_on_with_runtime<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::runtime::Runtime::new().unwrap().block_on(future)
}

/// executor stream abstractions
pub mod stream {
    /// executor strean timeout abstractions
    pub mod to {
        pub use tokio_stream::Timeout;
    }
}
/// executor future abstractions
pub mod future {
    /// executor future timeout abstractions
    pub mod to {
        pub use tokio::time::error::Elapsed as TimeoutError;
        pub use tokio::time::Timeout;
        /// Result from await of timeout on future
        pub type Result<T> = std::result::Result<T, tokio::time::error::Elapsed>;
    }
}

use tokio::runtime::Handle;

/// Provides `block_on` with `tokio` that matches `async_std::task::block_on`
pub fn async_block_on<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::task::block_in_place(move || Handle::current().block_on(future))
}

/// Splits a `TcpStream` into reader and writer
#[must_use]
pub fn split_stream(stream: TcpStream) -> (OwnedReadHalf, OwnedWriteHalf) {
    stream.into_split()
}
