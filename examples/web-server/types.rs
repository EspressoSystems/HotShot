use crate::infra::WebServerRun;
use hotshot::traits::implementations::MemoryStorage;
use hotshot::{
    demos::vdemo::VDemoTypes,
    traits::{
        election::static_committee::GeneralStaticCommittee,
        implementations::CentralizedWebCommChannel,
    },
};
use hotshot_types::message::Message;
use hotshot_types::traits::election::QuorumExchange;
use hotshot_types::traits::node_implementation::NodeImplementation;
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
pub type ThisNetwork =
    CentralizedWebCommChannel<VDemoTypes, NodeImpl, ThisProposal, ThisVote, ThisMembership>;
pub type ThisProposal = ValidatingProposal<VDemoTypes, ThisLeaf>;
pub type ThisVote = QuorumVote<VDemoTypes, ThisLeaf>;

impl NodeImplementation<VDemoTypes> for NodeImpl {
    type Storage = MemoryStorage<VDemoTypes, Self::Leaf>;
    type Leaf = ValidatingLeaf<VDemoTypes>;
    type QuorumExchange = QuorumExchange<
        VDemoTypes,
        Self::Leaf,
        ThisProposal,
        ThisMembership,
        ThisNetwork,
        Message<VDemoTypes, Self>,
    >;
    type CommitteeExchange = Self::QuorumExchange;
}
pub type ThisRun = WebServerRun<VDemoTypes, NodeImpl, ThisMembership>;
