use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use async_broadcast::Receiver;
use async_compatibility_layer::art::async_spawn;
use async_lock::RwLock;
use hotshot_types::{
    consensus::Consensus,
    data::VidCommitment,
    traits::{
        network::{DataRequest, RequestKind},
        node_implementation::{NodeImplementation, NodeType},
    },
};

/// Task for requesting information from the network
struct RequestStateTask<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    event_stream: Receiver<DataRequest<TYPES>>,
    quorum_membership: TYPES::Membership,
    da_mebership: TYPES::Membership,
    quorum_network: I::QuorumNetwork,
    da_network: I::CommitteeNetwork,
    vid_equests: BTreeMap<TYPES::Time, HashMap<VidCommitment, u64>>,
    consensus: Arc<RwLock<Consensus<TYPES>>>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> RequestStateTask<TYPES, I> {
    fn run_loop(mut self) {
        async_spawn(async move {
            while let Ok(event) = self.event_stream.recv().await {
                match event.request {
                    RequestKind::VID(commit, key) => todo!(),
                    RequestKind::DAProposal(view) => todo!(),
                }
            }
        });
    }
}
