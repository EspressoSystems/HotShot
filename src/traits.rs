/// Sortition trait
pub mod election;
mod networking;
mod node_implementation;
mod storage;

pub use hotshot_types::traits::{stateful_handler::StatefulHandler, BlockContents, StateContents};
pub use networking::{NetworkError, NetworkReliability, NetworkingImplementation};
pub use node_implementation::NodeImplementation;
pub use storage::{Result as StorageResult, Storage};

/// Module for publicly usable implementations of the traits
pub mod implementations {
    pub use super::{
        networking::{
            centralized_server_network::CentralizedServerNetwork,
            libp2p_network::{Libp2pNetwork, PeerInfoVec},
            memory_network::{DummyReliability, MasterMap, MemoryNetwork},
            w_network::WNetwork,
        },
        storage::memory_storage::MemoryStorage, // atomic_storage::AtomicStorage,
    };
    pub use hotshot_types::traits::stateful_handler::Stateless;
}

/// Dummy testing implementations
#[cfg(test)]
pub mod dummy {
    pub use hotshot_types::traits::state::dummy::DummyState;
}
