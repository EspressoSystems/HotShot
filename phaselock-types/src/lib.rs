//! Types and Traits for the `PhaseLock` consensus module
#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::panic
)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]
pub mod constants;
pub mod data;
pub mod error;
pub mod event;
pub mod message;
pub mod traits;
