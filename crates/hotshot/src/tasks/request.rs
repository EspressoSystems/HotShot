use async_broadcast::Receiver;
use async_compatibility_layer::channel::UnboundedReceiver;
use hotshot_task_impls::events::HotShotEvent;
use hotshot_types::traits::node_implementation::{NodeImplementation, NodeType};


/// Task for requesting information from the network
struct RequestStateTask<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    pub(crate) event_stream: Receiver<HotShotEvent<TYPES>>,
    pub(crate) quorum_membership: TYPES::Membership,
    pub(crate) da_mebership: TYPES::Membership,
    pub(crate) quorum_network: I::QuorumNetwork,
    pub(crate) da_network: I::CommitteeNetwork,
}

