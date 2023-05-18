//! Common traits for the `HotShot` protocol
pub mod block_contents;
pub mod consensus_type;
pub mod election;
pub mod metrics;
pub mod network;
pub mod node_implementation;
pub mod signature_key;
pub mod state;
pub mod storage;

pub use block_contents::Block;
pub use state::State;
