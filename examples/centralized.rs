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
type ThisMembership =
    GeneralStaticCommittee<VDemoTypes, ThisLeaf, <VDemoTypes as NodeType>::SignatureKey>;
type ThisNetwork = CentralizedCommChannel<VDemoTypes, ThisProposal, ThisVote, ThisMembership>;
type ThisProposal = ValidatingProposal<VDemoTypes, ThisLeaf>;
type ThisVote = QuorumVote<VDemoTypes, ThisLeaf>;
type ThisNode = VDemoNode<ThisNetwork, ThisMembership>;
type ThisConfig = CentralizedConfig<VDemoTypes, ThisMembership>;

#[cfg_attr(
    feature = "tokio-executor",
    tokio::main(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::main)]
#[instrument]
async fn main() {
    let args = CliOrchestrated::parse();

    main_entry_point::<VDemoTypes, ThisMembership, ThisNetwork, ThisNode, ThisConfig>(args).await;
}
