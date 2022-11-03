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

#[cfg(all(feature = "async-std-executor", feature = "tokio-executor"))]
std::compile_error!("Both feature \"async-std-executor\" and feature \"tokio-executor\" must not be concurrently enabled for this crate.");

#[cfg(not(any(feature = "async-std-executor", feature = "tokio-executor")))]
compile_error! {"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}

/// abstraction over both `tokio` and `async-std`, making it possible to use either based on a feature flag
#[cfg(feature = "async-std-executor")]
#[path = "art/async-std.rs"]
pub mod art;

/// abstraction over both `tokio` and `async-std`, making it possible to use either based on a feature flag
#[cfg(feature = "tokio-executor")]
#[path = "art/tokio.rs"]
pub mod art;

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
