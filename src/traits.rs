mod block_contents;
/// Sortition trait
pub mod election;
mod networking;
mod node_implementation;
mod state;
mod stateful_handler;
mod storage;

pub use block_contents::BlockContents;
pub use election::Election;
pub use networking::{BoxedFuture, NetworkError, NetworkReliability, NetworkingImplementation};
pub use node_implementation::NodeImplementation;
pub use state::State;
pub use stateful_handler::StatefulHandler;
pub use storage::{Storage, StorageResult};

/// Module for publicly usable implementations of the traits
pub mod implementations {
    pub use super::networking::memory_network::{DummyReliability, MasterMap, MemoryNetwork};
    pub use super::networking::w_network::WNetwork;
    pub use super::stateful_handler::Stateless;
    pub use super::storage::memory_storage::MemoryStorage;
}

/// Dummy testing implementations
#[cfg(test)]
pub mod dummy {
    pub use super::state::dummy::DummyState;
}
