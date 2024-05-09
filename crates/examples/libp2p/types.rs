use std::fmt::Debug;

use hotshot::traits::implementations::Libp2pNetwork;
use hotshot_example_types::{state_types::TestTypes, storage_types::TestStorage};
use hotshot_types::{
    message::Message,
    traits::node_implementation::{NodeImplementation, NodeType},
};
use serde::{Deserialize, Serialize};

use crate::infra::Libp2pDARun;

/// dummy struct so we can choose types
#[derive(Clone, Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct NodeImpl {}

/// convenience type alias
pub type DANetwork = Libp2pNetwork<Message<TestTypes>, <TestTypes as NodeType>::SignatureKey>;
/// convenience type alias
pub type QuorumNetwork = Libp2pNetwork<Message<TestTypes>, <TestTypes as NodeType>::SignatureKey>;

impl NodeImplementation<TestTypes> for NodeImpl {
    type QuorumNetwork = QuorumNetwork;
    type DANetwork = DANetwork;
    type Storage = TestStorage<TestTypes>;
}
/// convenience type alias
pub type ThisRun = Libp2pDARun<TestTypes>;
