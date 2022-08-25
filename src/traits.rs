/// Sortition trait
pub mod election;
mod networking;
mod node_implementation;
mod storage;

pub use hotshot_types::traits::{
    stateful_handler::StatefulHandler, BlockContents, StateContents,
};
pub use networking::{NetworkError, NetworkReliability, NetworkingImplementation};
pub use node_implementation::NodeImplementation;
pub use storage::{Storage, StorageResult};

/// Module for publicly usable implementations of the traits
pub mod implementations {
    pub use super::{
        networking::{
            libp2p_network::{Libp2pNetwork, PeerInfoVec},
            memory_network::{DummyReliability, MasterMap, MemoryNetwork},
            w_network::WNetwork,
        },
        storage::{atomic_storage::AtomicStorage, memory_storage::MemoryStorage},
    };
    pub use hotshot_types::traits::stateful_handler::Stateless;
}

/// Dummy testing implementations
#[cfg(test)]
pub mod dummy {
    pub use hotshot_types::traits::state::dummy::DummyState;
}
