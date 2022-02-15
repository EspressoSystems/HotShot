/// Sortition trait
pub mod election;
mod networking;
mod node_implementation;
mod storage;

pub use election::Election;
pub use networking::{BoxedFuture, NetworkError, NetworkReliability, NetworkingImplementation};
pub use node_implementation::NodeImplementation;
pub use phaselock_types::traits::stateful_handler::StatefulHandler;
pub use phaselock_types::traits::BlockContents;
pub use phaselock_types::traits::State;
pub use storage::{Storage, StorageResult};

/// Module for publicly usable implementations of the traits
pub mod implementations {
    pub use super::networking::memory_network::{DummyReliability, MasterMap, MemoryNetwork};
    pub use super::networking::w_network::WNetwork;
    pub use super::storage::memory_storage::MemoryStorage;
    pub use phaselock_types::traits::stateful_handler::Stateless;
}

/// Dummy testing implementations
#[cfg(test)]
pub mod dummy {
    pub use phaselock_types::traits::state::dummy::DummyState;
}
