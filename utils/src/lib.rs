//! Contains general utility structures and methods

#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::panic
)]

/// Provides an unbounded size broadcast async-aware queue
pub mod broadcast;
/// A mutex that can be subscribed to, and will notify the subscribers whenever the internal data is changed.
pub mod subscribable_mutex;

/// A rwlock that can be subscribed to, and will return state to subscribers whenever the internal
/// data is changed.
pub mod subscribable_rwlock;

/// Provides types useful for waiting on certain values to arrive
// pub mod waitqueue;

/// Provides bincode options
pub mod bincode;

/// Collection of hacks
pub mod hack {
    /// Satisfies type checker without breaking non lexical lifetimes
    /// # Panics
    /// Always panics.
    #[deprecated]
    pub fn nll_todo<S>() -> S {
        None.unwrap()
    }
}

cfg_if::cfg_if! {
    if #[cfg(all(feature = "async-std-executor", feature = "tokio-executor"))] {
        std::compile_error!{"Both feature \"async-std-executor\" and feature \"tokio-executor\" must not be concurrently enabled for this crate."}
    } else if #[cfg(feature = "async-std-executor")] {
        /// async runtime/executor symmetric wrappers, `async-std` edition
        pub mod art {
            use std::future::Future;
            use std::time::Duration;
            use async_std::prelude::FutureExt;
            pub use async_std::{
                main as async_main,
                task::{block_on as async_block_on, sleep as async_sleep, spawn as async_spawn, spawn_local as async_spawn_local, block_on as async_block_on_with_runtime},
                test as async_test,
                net::TcpStream,
                io::{WriteExt as AsyncWriteExt, ReadExt as AsyncReadExt},
            };
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
        }
    } else if #[cfg(feature = "tokio-executor")] {
        /// async runtime/executor symmetric wrappers, `tokio` edition
        pub mod art {
            use tokio::net::{tcp::OwnedReadHalf, tcp::OwnedWriteHalf};
            pub use tokio::{
                main as async_main,
                task::spawn as async_spawn_local,
                test as async_test,
                time::{sleep as async_sleep, timeout as async_timeout},
                net::TcpStream,
                io::{AsyncWriteExt, AsyncReadExt},
            };


            cfg_if::cfg_if! {
                if #[cfg(feature = "profiling")] {
                    /// spawn and log task id
                    pub fn async_spawn<T>(future: T) -> tokio::task::JoinHandle<T::Output>
                        where
                        T: futures::Future + Send + 'static,
                        T::Output: Send + 'static, {
                            let async_span = tracing::error_span!("Task Root", tsk_id = tracing::field::Empty);
                            let join_handle = tokio::task::spawn(tracing::Instrument::instrument(async{future.await}, async_span.clone()));
                            async_span.record("tsk_id", tracing::field::display(join_handle.id()));
                            join_handle
                        }
                } else {
                    pub use tokio::spawn as async_spawn;
                }
            }

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
        }
    } else {
        std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
    }
}

pub mod channel;

/// Provides utility functions used for testing
#[cfg(feature = "logging-utils")]
pub mod test_util {
    // turn off some lints, this is a janky test method
    #![allow(
        clippy::panic,
        clippy::redundant_closure_for_method_calls,
        clippy::missing_panics_doc
    )]
    use std::{
        env::{var, VarError},
        sync::Once,
    };
    use tracing_error::ErrorLayer;
    use tracing_subscriber::{fmt::format::FmtSpan, EnvFilter, Registry};

    /// Ensure logging is only
    /// initialized once
    static INIT: Once = Once::new();

    /// Ensure backtrace is only
    /// initialized once
    static INIT_2: Once = Once::new();

    /// enable backtraces exactly once
    pub fn setup_backtrace() {
        INIT_2.call_once(|| {
            color_eyre::install().unwrap();
        });
    }

    #[cfg(feature = "profiling")]
    /// generate the open telemetry layer
    /// and set the global propagator
    fn gen_opentelemetry_layer() -> opentelemetry::sdk::trace::Tracer {
        use opentelemetry::{
            sdk::{
                trace::{RandomIdGenerator, Sampler},
                Resource,
            },
            KeyValue,
        };
        opentelemetry::global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());
        opentelemetry_jaeger::new_agent_pipeline()
            .with_service_name("HotShot Tracing")
            .with_auto_split_batch(true)
            .with_trace_config(
                opentelemetry::sdk::trace::config()
                    .with_sampler(Sampler::AlwaysOn)
                    .with_id_generator(RandomIdGenerator::default())
                    .with_max_events_per_span(64)
                    .with_max_attributes_per_span(64)
                    .with_max_events_per_span(64)
                    // resources will translated to tags in jaeger spans
                    .with_resource(Resource::new(vec![
                        KeyValue::new("key", "value"),
                        KeyValue::new("process_key", "process_value"),
                    ])),
            )
            // TODO make this toggle-able between tokio and async-std
            .install_batch(opentelemetry::runtime::Tokio)
            // TODO make endpoint configurable
            // .with_endpoint("http://localhost:14250/api/trace")
            .unwrap()
    }

    /// complete init of the tracer
    /// this is needed because the types are janky
    /// I couldn't get the types to play nicely with a generic function
    macro_rules! complete_init {
        ( $R:expr ) => {
            #[cfg(feature = "tokio-executor")]
            let console_layer = var("TOKIO_CONSOLE_ENABLED") == Ok("true".to_string());

            #[cfg(feature = "profiling")]
            let tracer_enabled = var("OTL_ENABLED") == Ok("true".to_string());

            #[cfg(all(feature = "tokio-executor", feature = "profiling"))]
            if console_layer && tracer_enabled {
                let registry = $R.with(console_subscriber::spawn());
                let registry = registry
                    .with(tracing_opentelemetry::layer().with_tracer(gen_opentelemetry_layer()));
                registry.init();
                return;
            }

            #[cfg(feature = "profiling")]
            if tracer_enabled {
                $R.with(tracing_opentelemetry::layer().with_tracer(gen_opentelemetry_layer()))
                    .init();
                return;
            }

            #[cfg(feature = "tokio-executor")]
            if console_layer {
                $R.with(console_subscriber::spawn()).init();
                return;
            }

            $R.init();
        };
    }

    /// Set up logging exactly once
    #[allow(clippy::too_many_lines)]
    pub fn setup_logging() {
        use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

        INIT.call_once(|| {
            let internal_event_filter =
                match var("RUST_LOG_SPAN_EVENTS") {
                    Ok(value) => {
                        value
                            .to_ascii_lowercase()
                            .split(',')
                            .map(|filter| match filter.trim() {
                                "new" => FmtSpan::NEW,
                                "enter" => FmtSpan::ENTER,
                                "exit" => FmtSpan::EXIT,
                                "close" => FmtSpan::CLOSE,
                                "active" => FmtSpan::ACTIVE,
                                "full" => FmtSpan::FULL,
                                _ => panic!("test-env-log: RUST_LOG_SPAN_EVENTS must contain filters separated by `,`.\n\t\
                                            For example: `active` or `new,close`\n\t\
                                             Supported filters: new, enter, exit, close, active, full\n\t\
                                             Got: {value}"),
                            })
                        .fold(FmtSpan::NONE, |acc, filter| filter | acc)
                    },
                    Err(VarError::NotUnicode(_)) =>
                        panic!("test-env-log: RUST_LOG_SPAN_EVENTS must contain a valid UTF-8 string"),
                    Err(VarError::NotPresent) => FmtSpan::NONE,
                };
            let fmt_env = var("RUST_LOG_FORMAT").map(|x| x.to_lowercase());


            match fmt_env.as_deref().map(|x| x.trim()) {
                Ok("full") => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .with_ansi(true)
                        .with_test_writer();
                    let registry = Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer);

                    complete_init!(registry);

                },
                Ok("json") => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .json()
                        .with_test_writer();
                    let registry = Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer);
                    complete_init!(registry);
                },
                Ok("compact") => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .with_ansi(true)
                        .compact()
                        .with_test_writer();
                    let registry = Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer);
                    complete_init!(registry);
                },
                _ => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .with_ansi(true)
                        .pretty()
                        .with_test_writer();
                    let registry = Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer);
                    complete_init!(registry);
                },
            };

            std::panic::set_hook(Box::new(|info| {
                tracing::error!(?info, "Thread panicked!");
                #[cfg(feature = "profiling")]
                opentelemetry::global::shutdown_tracer_provider();
            }));
        });
    }

    /// shuts down logging
    pub fn shutdown_logging() {
        #[cfg(feature = "profiling")]
        opentelemetry::global::shutdown_tracer_provider();
    }
}
