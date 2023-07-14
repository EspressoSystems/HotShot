use crate::infra_da::WebServerDARun;
use hotshot::traits::implementations::MemoryStorage;
use hotshot::{
    demos::sdemo::SDemoTypes, demos::vdemo::VDemoTypes,
    traits::{election::static_committee::GeneralStaticCommittee, implementations::WebCommChannel},
};
use hotshot_types::data::{ValidatingProposal, ValidatingLeaf};
use hotshot_types::message::Message;
use hotshot_types::traits::node_implementation::ViewSyncProposalType;
use hotshot_types::traits::{
    election::{CommitteeExchange, QuorumExchange},
    node_implementation::{NodeImplementation, SequencingExchanges},
};
use hotshot_types::{
    data::{DAProposal, QuorumProposal, SequencingLeaf},
    message::SequencingMessage,
    traits::{node_implementation::NodeType, election::ViewSyncExchange, consensus_type::{validating_consensus::ValidatingConsensus, sequencing_consensus::SequencingConsensus}},
    vote::{DAVote, QuorumVote, ViewSyncVote},
    certificate::ViewSyncCertificate,
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
    WebCommChannel<SDemoTypes, NodeImpl, ThisValidatingProposal, ThisViewSyncVote, ThisMembership>;

pub type ThisDAProposal = DAProposal<SDemoTypes>;
pub type ThisDAVote = DAVote<SDemoTypes>;

pub type ThisQuorumProposal = QuorumProposal<SDemoTypes, ThisLeaf>;
pub type ThisQuorumVote = QuorumVote<SDemoTypes, ThisLeaf>;

pub type ThisValidatingProposal = ValidatingProposal<SDemoTypes, ValidatingLeaf<SDemoTypes>>;
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
        ViewSyncExchange<SDemoTypes, ThisValidatingProposal, ThisMembership, ViewSyncNetwork, Message<SDemoTypes, Self>>,
    >;
    type ConsensusMessage = SequencingMessage<SDemoTypes, Self>;

    fn new_channel_maps(
        start_view: ViewNumber,
    ) -> (
        ChannelMaps<VDemoTypes, Self>,
        Option<ChannelMaps<SDemoTypes, Self>>,
    ) {
        (ChannelMaps::new(start_view), None)
    }
}
pub type ThisRun = WebServerDARun<SDemoTypes, NodeImpl, ThisMembership>;