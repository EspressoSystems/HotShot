use crate::infra_da::WebServerDARun;
use hotshot::traits::implementations::MemoryStorage;
use hotshot::{
    demos::sdemo::SDemoTypes,
    traits::{election::static_committee::GeneralStaticCommittee, implementations::WebCommChannel},
};
use hotshot_types::data::{ValidatingLeaf, ValidatingProposal};
use hotshot_types::message::Message;
use hotshot_types::traits::{
    election::{CommitteeExchange, QuorumExchange},
    node_implementation::{NodeImplementation, SequencingExchanges},
};
use hotshot_types::{
    certificate::ViewSyncCertificate,
    data::{DAProposal, QuorumProposal, SequencingLeaf, ViewNumber},
    message::SequencingMessage,
    traits::{
        consensus_type::{
            sequencing_consensus::SequencingConsensus, validating_consensus::ValidatingConsensus,
        },
        election::ViewSyncExchange,
        node_implementation::{ChannelMaps, NodeType},
    },
    vote::{DAVote, QuorumVote, ViewSyncVote},
};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[derive(Clone, Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct NodeImpl {}

pub type ThisLeaf = SequencingLeaf<SDemoTypes>;
pub type ThisMembership =
    GeneralStaticCommittee<SDemoTypes, ThisLeaf, <SDemoTypes as NodeType>::SignatureKey>;
pub type DANetwork =
    WebCommChannel<SDemoTypes, NodeImpl, ThisDAProposal, ThisDAVote, ThisMembership>;
pub type QuorumNetwork =
    WebCommChannel<SDemoTypes, NodeImpl, ThisQuorumProposal, ThisQuorumVote, ThisMembership>;
pub type ViewSyncNetwork =
    WebCommChannel<SDemoTypes, NodeImpl, ThisViewSyncProposal, ThisViewSyncVote, ThisMembership>;

pub type ThisDAProposal = DAProposal<SDemoTypes>;
pub type ThisDAVote = DAVote<SDemoTypes>;

pub type ThisQuorumProposal = QuorumProposal<SDemoTypes, ThisLeaf>;
pub type ThisQuorumVote = QuorumVote<SDemoTypes, ThisLeaf>;

pub type ThisViewSyncProposal = ViewSyncCertificate<SDemoTypes>;
pub type ThisViewSyncVote = ViewSyncVote<SDemoTypes>;

impl NodeImplementation<SDemoTypes> for NodeImpl {
    type Storage = MemoryStorage<SDemoTypes, Self::Leaf>;
    type Leaf = SequencingLeaf<SDemoTypes>;
    type Exchanges = SequencingExchanges<
        SDemoTypes,
        Message<SDemoTypes, Self>,
        QuorumExchange<
            SDemoTypes,
            Self::Leaf,
            ThisQuorumProposal,
            ThisMembership,
            QuorumNetwork,
            Message<SDemoTypes, Self>,
        >,
        CommitteeExchange<SDemoTypes, ThisMembership, DANetwork, Message<SDemoTypes, Self>>,
        ViewSyncExchange<
            SDemoTypes,
            ThisViewSyncProposal,
            ThisMembership,
            ViewSyncNetwork,
            Message<SDemoTypes, Self>,
        >,
    >;
    type ConsensusMessage = SequencingMessage<SDemoTypes, Self>;

    fn new_channel_maps(
        start_view: ViewNumber,
    ) -> (
        ChannelMaps<SDemoTypes, Self>,
        Option<ChannelMaps<SDemoTypes, Self>>,
    ) {
        (ChannelMaps::new(start_view), None)
    }
}
pub type ThisRun = WebServerDARun<SDemoTypes, NodeImpl, ThisMembership>;
