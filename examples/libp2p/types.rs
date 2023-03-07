use crate::infra::Libp2pRun;
use hotshot::{
    demos::vdemo::{VDemoNode, VDemoTypes},
    traits::{
        election::static_committee::GeneralStaticCommittee, implementations::Libp2pCommChannel,
    },
};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    traits::node_implementation::NodeType,
    vote::QuorumVote,
};

pub type ThisLeaf = ValidatingLeaf<VDemoTypes>;
pub type ThisMembership =
    GeneralStaticCommittee<VDemoTypes, ThisLeaf, <VDemoTypes as NodeType>::SignatureKey>;
pub type ThisNetwork = Libp2pCommChannel<VDemoTypes, ThisProposal, ThisVote, ThisMembership>;
pub type ThisProposal = ValidatingProposal<VDemoTypes, ThisLeaf>;
pub type ThisVote = QuorumVote<VDemoTypes, ThisLeaf>;
pub type ThisNode = VDemoNode<ThisNetwork, ThisMembership>;
pub type ThisRun = Libp2pRun<VDemoTypes, ThisMembership>;
