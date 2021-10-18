/// `BlockContents` trait
pub mod block_contents;
/// Sortition trait
pub mod election;
/// `NodeImplementation` trait
pub mod node_implementation;
/// `State` trait
pub mod state;
/// Stateful handler callback trait
pub mod stateful_handler;
/// `Storage` trait
pub mod storage;

pub use block_contents::BlockContents;
pub use election::Election;
pub use node_implementation::NodeImplementation;
pub use state::State;
pub use stateful_handler::StatefulHandler;
pub use storage::Storage;
