use crate::infra::WebServerDARun;
use hotshot::traits::implementations::{MemoryStorage, WebServerNetwork};
use hotshot_example_types::state_types::TestTypes;
use hotshot_types::traits::node_implementation::NodeImplementation;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// dummy struct so we can choose types
#[derive(Clone, Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct NodeImpl {}

/// convenience type alias
pub type DANetwork = WebServerNetwork<TestTypes, 0, 1>;
/// convenience type alias
pub type VIDNetwork = WebServerNetwork<TestTypes, 0, 1>;
/// convenience type alias
pub type QuorumNetwork = WebServerNetwork<TestTypes, 0, 1>;
/// convenience type alias
pub type ViewSyncNetwork = WebServerNetwork<TestTypes, 0, 1>;

impl NodeImplementation<TestTypes> for NodeImpl {
    type Storage = MemoryStorage<TestTypes>;
    type CommitteeNetwork = DANetwork;
    type QuorumNetwork = QuorumNetwork;
}
/// convenience type alias
pub type ThisRun = WebServerDARun<TestTypes, 0, 1>;
