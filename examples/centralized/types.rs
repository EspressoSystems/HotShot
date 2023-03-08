use hotshot::traits::implementations::MemoryStorage;
use hotshot::{
    demos::vdemo::{VDemoNode, VDemoTypes},
    traits::{
        election::static_committee::GeneralStaticCommittee, implementations::CentralizedCommChannel,
    },
};
use hotshot_types::message::Message;
use hotshot_types::traits::election::QuorumExchange;
use hotshot_types::traits::election::{CommitteeExchange, Membership};
use hotshot_types::traits::node_implementation::NodeImplementation;
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    traits::node_implementation::NodeType,
    vote::QuorumVote,
};
use std::fmt::Debug;

use crate::infra::CentralizedConfig;

#[derive(Clone, Debug)]
pub struct NodeImpl {}

pub type ThisLeaf = ValidatingLeaf<VDemoTypes>;
pub type ThisMembership =
    GeneralStaticCommittee<VDemoTypes, ThisLeaf, <VDemoTypes as NodeType>::SignatureKey>;
pub type ThisNetwork =
    CentralizedCommChannel<VDemoTypes, ThisNode, ThisProposal, ThisVote, ThisMembership>;
pub type ThisProposal = ValidatingProposal<VDemoTypes, ThisLeaf>;
pub type ThisVote = QuorumVote<VDemoTypes, ThisLeaf>;
pub type ThisNode = NodeImpl;
pub type ThisConfig = CentralizedConfig<VDemoTypes, ThisNode, ThisMembership>;

impl NodeImplementation<VDemoTypes> for NodeImpl {
    type Storage = MemoryStorage<VDemoTypes, Self::Leaf>;
    type Leaf = ValidatingLeaf<VDemoTypes>;
    type QuorumExchange = QuorumExchange<VDemoTypes, Self::Leaf, ThisProposal, ThisMembership, ThisNetwork, Message<VDemoTypes, Self>>;
    type ComitteeExchange = Self::QuorumExchange;
}
