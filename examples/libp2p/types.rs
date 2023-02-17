use crate::infra::Libp2pClientConfig;
use hotshot::{
    demos::vdemo::{VDemoNode, VDemoTypes},
    traits::{
        election::static_committee::GeneralStaticCommittee, implementations::Libp2pCommChannel,
    },
};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    message::QuorumVote,
    traits::node_implementation::NodeType,
};

pub type ThisLeaf = ValidatingLeaf<VDemoTypes>;
pub type ThisElection =
    GeneralStaticCommittee<VDemoTypes, ThisLeaf, <VDemoTypes as NodeType>::SignatureKey>;
pub type ThisNetwork = Libp2pCommChannel<VDemoTypes, ThisProposal, ThisVote, ThisElection>;
pub type ThisProposal = ValidatingProposal<VDemoTypes, ThisElection>;
pub type ThisVote = QuorumVote<VDemoTypes, ThisLeaf>;
pub type ThisNode = VDemoNode<ThisNetwork, ThisElection>;
pub type ThisConfig = Libp2pClientConfig<VDemoTypes, ThisElection>;
