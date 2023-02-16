use clap::Parser;
use hotshot::{
    demos::vdemo::{VDemoNode, VDemoTypes},
    traits::{
        election::static_committee::GeneralStaticCommittee, implementations::CentralizedCommChannel,
    },
};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    message::QuorumVote,
    traits::node_implementation::NodeType,
};
use infra::CentralizedConfig;
use tracing::instrument;

use crate::infra::{main_entry_point, CliOrchestrated};

pub mod infra;

type ThisLeaf = ValidatingLeaf<VDemoTypes>;
type ThisElection =
    GeneralStaticCommittee<VDemoTypes, ThisLeaf, <VDemoTypes as NodeType>::SignatureKey>;
type ThisNetwork = CentralizedCommChannel<VDemoTypes, ThisProposal, ThisVote, ThisElection>;
type ThisProposal = ValidatingProposal<VDemoTypes, ThisElection>;
type ThisVote = QuorumVote<VDemoTypes, ThisLeaf>;
type ThisNode = VDemoNode<ThisNetwork, ThisElection>;
type ThisConfig = CentralizedConfig<VDemoTypes, ThisElection>;

#[cfg_attr(
    feature = "tokio-executor",
    tokio::main(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::main)]
#[instrument]
async fn main() {
    let args = CliOrchestrated::parse();

    main_entry_point::<VDemoTypes, ThisElection, ThisNetwork, ThisNode, ThisConfig>(args).await;
}
