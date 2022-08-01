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
    use tracing_subscriber::{
        fmt::{self, format::FmtSpan},
        prelude::*,
        EnvFilter, Registry,
    };

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

    /// Set up logging exactly once
    pub fn setup_logging() {
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
                                             Got: {}", value),
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
                    Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer)
                        .init();
                },
                Ok("json") => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .json()
                        .with_test_writer();
                    Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer)
                        .init();
                },
                Ok("compact") => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .with_ansi(true)
                        .compact()
                        .with_test_writer();
                    Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer)
                        .init();
                },
                _ => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .with_ansi(true)
                        .pretty()
                        .with_test_writer();
                    Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer)
                        .init();
                },
            };
        });
    }
}
