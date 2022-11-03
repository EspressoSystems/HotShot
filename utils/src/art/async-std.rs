use async_std::prelude::FutureExt;
pub use async_std::{
    io::{ReadExt as AsyncReadExt, WriteExt as AsyncWriteExt},
    main as async_main,
    net::TcpStream,
    task::{
        block_on as async_block_on, block_on as async_block_on_with_runtime, sleep as async_sleep,
        spawn as async_spawn, spawn_local as async_spawn_local,
    },
    test as async_test,
};
use std::future::Future;
use std::time::Duration;
/// executor stream abstractions
pub mod stream {
    /// executor strean timeout abstractions
    pub mod to {
        pub use async_std::stream::Timeout;
    }
}
/// executor future abstractions
pub mod future {
    /// executor future timeout abstractions
    pub mod to {
        pub use async_std::future::TimeoutError;
        /// Result from await of timeout on future
        pub type Result<T> = std::result::Result<T, async_std::future::TimeoutError>;
    }
}

/// Provides timeout with `async_std` that matches `tokio::time::timeout`
pub fn async_timeout<T>(
    duration: Duration,
    future: T,
) -> impl Future<Output = Result<T::Output, async_std::future::TimeoutError>>
where
    T: Future,
{
    future.timeout(duration)
}

/// Splits a `TcpStream` into reader and writer
#[must_use]
pub fn split_stream(stream: TcpStream) -> (TcpStream, TcpStream) {
    (stream.clone(), stream)
}
