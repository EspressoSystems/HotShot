//! Common traits for the `PhaseLock` protocol
pub mod block_contents;
pub mod network;
pub mod node_implementation;
pub mod signature_key;
pub mod state;
pub mod stateful_handler;
pub mod storage;

pub use block_contents::BlockContents;
pub use state::State;
