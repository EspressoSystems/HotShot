#![allow(clippy::pedantic, clippy::panic)]
use std::env::{var, VarError};
use tracing_error::ErrorLayer;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan, writer::BoxMakeWriter},
    prelude::*,
    EnvFilter, Registry,
};

fn parse_span_events() -> FmtSpan {
    match var("RUST_LOG_SPAN_EVENTS") {
        Ok(value) => value
            .to_ascii_lowercase()
            .split(',')
            .map(|filter| match filter.trim() {
                "new" => FmtSpan::NEW,
                "enter" => FmtSpan::ENTER,
                "exit" => FmtSpan::EXIT,
                "close" => FmtSpan::CLOSE,
                "active" => FmtSpan::ACTIVE,
                "full" => FmtSpan::FULL,
                _ => panic!(
                    "test-env-log: RUST_LOG_SPAN_EVENTS must contain filters separated by `,`.\n\t\
                                 For example: `active` or `new,close`\n\t\
                                 Supported filters: new, enter, exit, close, active, full\n\t\
                                 Got: {}",
                    value
                ),
            })
            .fold(FmtSpan::NONE, |acc, filter| filter | acc),
        Err(VarError::NotUnicode(_)) => {
            panic!("test-env-log: RUST_LOG_SPAN_EVENTS must contain a valid UTF-8 string")
        }
        Err(VarError::NotPresent) => FmtSpan::NONE,
    }
}

fn parse_writer() -> BoxMakeWriter {
    let file = var("RUST_LOG_OUTPUT").map(|x| x.trim().to_lowercase());
    match file.as_deref() {
        Ok("stderr") => BoxMakeWriter::new(std::io::stderr),
        Ok("stdout") => BoxMakeWriter::new(std::io::stdout),
        Ok(_) => panic!("Invalid RUST_LOG_OUTPUT value"),
        Err(_) => BoxMakeWriter::new(std::io::stderr),
    }
}

fn internal_setup_tracing(writer: BoxMakeWriter) {
    let internal_event_filter = parse_span_events();
    let fmt_env = var("RUST_LOG_FORMAT").map(|x| x.to_lowercase());
    match fmt_env.as_deref().map(|x| x.trim()) {
        Ok("full") => {
            let fmt_layer = fmt::Layer::default()
                .with_span_events(internal_event_filter)
                .with_writer(writer);
            let _subscriber = Registry::default()
                .with(EnvFilter::from_default_env())
                .with(ErrorLayer::default())
                .with(fmt_layer)
                .init();
        }
        Ok("json") => {
            let fmt_layer = fmt::Layer::default()
                .with_span_events(internal_event_filter)
                .json()
                .with_writer(writer);
            let _subscriber = Registry::default()
                .with(EnvFilter::from_default_env())
                .with(ErrorLayer::default())
                .with(fmt_layer)
                .init();
        }
        Ok("compact") => {
            let fmt_layer = fmt::Layer::default()
                .with_span_events(internal_event_filter)
                .compact()
                .with_writer(writer);
            let _subscriber = Registry::default()
                .with(EnvFilter::from_default_env())
                .with(ErrorLayer::default())
                .with(fmt_layer)
                .init();
        }
        _ => {
            let fmt_layer = fmt::Layer::default()
                .with_span_events(internal_event_filter)
                .with_ansi(true)
                .pretty()
                .with_writer(writer);
            let _subscriber = Registry::default()
                .with(EnvFilter::from_default_env())
                .with(ErrorLayer::default())
                .with(fmt_layer)
                .init();
        }
    };
}

pub fn setup_tracing() {
    let writer = parse_writer();
    internal_setup_tracing(writer);
}

pub fn setup_tracing_test() {
    let writer = BoxMakeWriter::new(fmt::writer::TestWriter::new());
    internal_setup_tracing(writer);
}
