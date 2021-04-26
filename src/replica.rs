use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::PubKey;

/// The ID for a replica
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ReplicaId;

/// The information for a replica
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ReplicaInfo {
    id: ReplicaId,
    // peer_id: PeerId,
    public_key: PubKey,
}

/// Configuration for a replica
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ReplicaConfig {
    replica_map: HashMap<ReplicaId, ReplicaInfo>,
    replicas: u64,
    majority: u64,
}

impl ReplicaConfig {
    pub fn add_replica(&mut self, id: ReplicaId, info: ReplicaInfo) {
        self.replica_map.insert(id, info);
        self.replicas += 1;
    }

    pub fn get_info(&self, id: ReplicaId) -> Option<&ReplicaInfo> {
        self.replica_map.get(&id)
    }

    pub fn get_pubkey(&self, id: ReplicaId) -> Option<&PubKey> {
        self.replica_map.get(&id).map(|x| &x.public_key)
    }

    // pub fn get_peerid(&self, id: ReplicaId) -> Option<&PeerId> {
    //     self.replica_map.get(&id).map(|x| x.peer_id )
    // }
}

impl Default for ReplicaConfig {
    fn default() -> Self {
        Self {
            replica_map: HashMap::new(),
            replicas: 0,
            majority: 0,
        }
    }
}
