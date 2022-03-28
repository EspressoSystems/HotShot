#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    missing_docs,
    // clippy::missing_docs_in_private_items,
    clippy::panic
)]
#![allow(
    clippy::option_if_let_else,
    clippy::must_use_candidate,
    clippy::module_name_repetitions,
    clippy::similar_names,
    clippy::unused_self
)]
//! Library for p2p communication

/// Direct Messages between two nodes
pub mod direct_message;

/// Wrapper for tracing niceties
pub mod tracing_setup;

/// Example message used by the UI library
pub mod message;

// /// UI library for clichat example
// TODO uncomment
// pub mod ui;

/// Network logic
pub mod network;
