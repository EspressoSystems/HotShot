use hotshot::{
    demos::vdemo::{VDemoNode, VDemoTypes},
    traits::{
        election::static_committee::GeneralStaticCommittee, implementations::CentralizedCommChannel,
    },
};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    traits::node_implementation::NodeType,
    vote::QuorumVote,
};

use crate::infra::CentralizedConfig;

pub type ThisLeaf = ValidatingLeaf<VDemoTypes>;
pub type ThisElection =
    GeneralStaticCommittee<VDemoTypes, ThisLeaf, <VDemoTypes as NodeType>::SignatureKey>;
pub type ThisNetwork = CentralizedCommChannel<VDemoTypes, ThisProposal, ThisVote, ThisElection>;
pub type ThisProposal = ValidatingProposal<VDemoTypes, ThisElection>;
pub type ThisVote = QuorumVote<VDemoTypes, ThisLeaf>;
pub type ThisNode = VDemoNode<ThisNetwork, ThisElection>;
pub type ThisConfig = CentralizedConfig<VDemoTypes, ThisElection>;
