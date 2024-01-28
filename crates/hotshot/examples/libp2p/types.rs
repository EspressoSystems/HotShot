use crate::infra::Libp2pDARun;
use hotshot::traits::implementations::{Libp2pRegularCommChannel, MemoryStorage};
use hotshot_testing::state_types::TestTypes;
use hotshot_types::traits::node_implementation::{ChannelMaps, NodeImplementation, NodeType};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// dummy struct so we can choose types
#[derive(Clone, Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct NodeImpl {}

/// convenience type alias
pub type DANetwork = Libp2pRegularCommChannel<TestTypes>;
/// convenience type alias
pub type VIDNetwork = Libp2pRegularCommChannel<TestTypes>;
/// convenience type alias
pub type QuorumNetwork = Libp2pRegularCommChannel<TestTypes>;
/// convenience type alias
pub type ViewSyncNetwork = Libp2pRegularCommChannel<TestTypes>;

impl NodeImplementation<TestTypes> for NodeImpl {
    type Storage = MemoryStorage<TestTypes>;
    type QuorumNetwork = QuorumNetwork;
    type CommitteeNetwork = DANetwork;

    fn new_channel_maps(
        start_view: <TestTypes as NodeType>::Time,
    ) -> (ChannelMaps<TestTypes>, Option<ChannelMaps<TestTypes>>) {
        (ChannelMaps::new(start_view), None)
    }
}
/// convenience type alias
pub type ThisRun = Libp2pDARun<TestTypes>;
