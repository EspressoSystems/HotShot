//! Contains general utility structures and methods

#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::panic
)]
/// Provides types useful for waiting on certain values to arrive
// pub mod waitqueue;

/// Provides bincode options
pub mod bincode;

/// task abstraction for hotshot tasks
pub mod hotshot_task;
