use crate::infra::WebServerRun;
use hotshot::traits::implementations::MemoryStorage;
use hotshot::{
    demos::vdemo::VDemoTypes,
    traits::{election::static_committee::GeneralStaticCommittee, implementations::WebCommChannel},
};
use hotshot_types::message::Message;
use hotshot_types::traits::{
    consensus_type::validating_consensus::ValidatingConsensus,
    election::QuorumExchange,
    node_implementation::{NodeImplementation, ValidatingExchanges},
};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    traits::node_implementation::NodeType,
    vote::QuorumVote,
};
use std::fmt::Debug;

#[derive(Clone, Debug)]
pub struct NodeImpl {}

pub type ThisLeaf = ValidatingLeaf<VDemoTypes>;
pub type ThisMembership =
    GeneralStaticCommittee<VDemoTypes, ThisLeaf, <VDemoTypes as NodeType>::SignatureKey>;
pub type ThisNetwork = WebCommChannel<
    ValidaitngConsensus,
    VDemoTypes,
    NodeImpl,
    ThisProposal,
    ThisVote,
    ThisMembership,
>;
pub type ThisProposal = ValidatingProposal<VDemoTypes, ThisLeaf>;
pub type ThisVote = QuorumVote<VDemoTypes, ThisLeaf>;

impl NodeImplementation<VDemoTypes> for NodeImpl {
    type Storage = MemoryStorage<VDemoTypes, Self::Leaf>;
    type Leaf = ValidatingLeaf<VDemoTypes>;
    type Exchanges = ValidatingExchanges<
        ValidatingConsensus,
        VDemoTypes,
        ValidatingLeaf<VDemoTypes>,
        Message<VDemoTypes, Self>,
        QuorumExchange<
            VDemoTypes,
            Self::Leaf,
            ThisProposal,
            ThisMembership,
            ThisNetwork,
            Message<VDemoTypes, Self>,
        >,
    >;
}
pub type ThisRun = WebServerRun<VDemoTypes, NodeImpl, ThisMembership>;
