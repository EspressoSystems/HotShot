use crate::infra::CombinedDARun;
use hotshot::traits::implementations::{CombinedNetworks, MemoryStorage};
use hotshot_example_types::state_types::TestTypes;
use hotshot_types::traits::node_implementation::NodeImplementation;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// dummy struct so we can choose types
#[derive(Clone, Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct NodeImpl {}

/// convenience type alias
pub type DANetwork = CombinedNetworks<TestTypes, 0, 1>;
/// convenience type alias
pub type VIDNetwork = CombinedNetworks<TestTypes, 0, 1>;
/// convenience type alias
pub type QuorumNetwork = CombinedNetworks<TestTypes, 0, 1>;
/// convenience type alias
pub type ViewSyncNetwork = CombinedNetworks<TestTypes, 0, 1>;

impl NodeImplementation<TestTypes> for NodeImpl {
    type Storage = MemoryStorage<TestTypes>;
    type QuorumNetwork = QuorumNetwork;
    type CommitteeNetwork = DANetwork;
}
/// convenience type alias
pub type ThisRun = CombinedDARun<TestTypes, 0, 1>;
