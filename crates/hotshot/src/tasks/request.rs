use async_broadcast::Receiver;
use hotshot_task_impls::events::HotShotEvent;
use hotshot_types::traits::node_implementation::{NodeImplementation, NodeType};

/// Task for requesting information from the network
/// Requests for information are triggered by the `event_stream`
/// and canceled when they are stale.
struct RequestTask<TYPES: NodeType> {
    pub(crate) event_stream: Receiver<HotShotEvent<TYPES>>,
    pub(crate) quorum_membership: TYPES::Membership,
    pub(crate) da_mebership: TYPES::Membership,
    
}