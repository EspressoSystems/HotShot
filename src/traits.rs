/// Sortition trait
pub mod election;
mod networking;
mod node_implementation;
mod storage;

pub use hotshot_types::traits::{Block, State};
pub use networking::{NetworkError, NetworkReliability, NetworkingImplementation};
pub use node_implementation::NodeImplementation;
pub use storage::{Result as StorageResult, Storage};

/// Module for publicly usable implementations of the traits
pub mod implementations {
    #[cfg(feature = "async-std-executor")]
    pub use super::networking::w_network::WNetwork;

    pub use super::{
        networking::{
            centralized_server_network::CentralizedServerNetwork,
            centralized_web_server_network::CentralizedWebServerNetwork,
            libp2p_network::{Libp2pNetwork, PeerInfoVec},
            memory_network::{DummyReliability, MasterMap, MemoryNetwork},
        },
        storage::memory_storage::MemoryStorage, // atomic_storage::AtomicStorage,
    };
}

/// Dummy testing implementations
pub mod dummy {
    pub use hotshot_types::traits::state::dummy::DummyState;
}
