use std::env::{var, VarError};
use tracing_error::ErrorLayer;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    prelude::*,
    EnvFilter, Registry,
};

/// Configures and installs the tracing listener
pub fn setup_tracing() {
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
    let fmt_layer = fmt::Layer::default()
        .with_span_events(internal_event_filter)
        .with_writer(std::io::stderr)
        .pretty();
    Registry::default()
        .with(EnvFilter::from_default_env())
        .with(ErrorLayer::default())
        .with(fmt_layer)
        .init();
}
