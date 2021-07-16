/// Provides types useful for waiting on a value matching a particular key to arrive
pub mod waitmap;
/// Provides types useful for waiting on certain values to arrive
pub mod waitqueue;

/// Provides utility functions used for testing
#[cfg(test)]
pub mod test_util {
    use std::env::{var, VarError};
    use std::sync::Once;
    use tracing_error::ErrorLayer;
    use tracing_subscriber::{
        fmt::{self, format::FmtSpan},
        prelude::*,
        EnvFilter, Registry,
    };

    static INIT: Once = Once::new();

    pub fn setup_logging() {
        INIT.call_once(|| {
            let internal_event_filter =
                match var("RUST_LOG_SPAN_EVENTS") {
                    Ok(value) => {
                        value
                            .to_ascii_lowercase()
                            .split(",")
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
                    let _subscriber = Registry::default()
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
                    let _subscriber = Registry::default()
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
                    let _subscriber = Registry::default()
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
                    let _subscriber = Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer)
                        .init();
                },
            };
        });
    }
}
