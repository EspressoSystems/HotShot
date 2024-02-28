use crate::infra::WebServerDARun;
use hotshot::traits::implementations::{MemoryStorage, WebServerNetwork};
use hotshot_constants::{WEB_SERVER_MAJOR_VERSION, WEB_SERVER_MINOR_VERSION};
use hotshot_example_types::state_types::TestTypes;
use hotshot_types::traits::node_implementation::NodeImplementation;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

/// dummy struct so we can choose types
#[derive(Clone, Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct NodeImpl {}

/// convenience type alias
pub type DANetwork =
    WebServerNetwork<TestTypes, WEB_SERVER_MAJOR_VERSION, WEB_SERVER_MINOR_VERSION>;
/// convenience type alias
pub type VIDNetwork =
    WebServerNetwork<TestTypes, WEB_SERVER_MAJOR_VERSION, WEB_SERVER_MINOR_VERSION>;
/// convenience type alias
pub type QuorumNetwork =
    WebServerNetwork<TestTypes, WEB_SERVER_MAJOR_VERSION, WEB_SERVER_MINOR_VERSION>;
/// convenience type alias
pub type ViewSyncNetwork =
    WebServerNetwork<TestTypes, WEB_SERVER_MAJOR_VERSION, WEB_SERVER_MINOR_VERSION>;

impl NodeImplementation<TestTypes> for NodeImpl {
    type Storage = MemoryStorage<TestTypes>;
    type CommitteeNetwork = DANetwork;
    type QuorumNetwork = QuorumNetwork;
}
/// convenience type alias
pub type ThisRun = WebServerDARun<TestTypes, WEB_SERVER_MAJOR_VERSION, WEB_SERVER_MINOR_VERSION>;
