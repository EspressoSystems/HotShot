use clap::Parser;
use hotshot::demos::vdemo::VDemoNode;
use hotshot::demos::vdemo::VDemoTypes;
use hotshot::traits::election::static_committee::GeneralStaticCommittee;
use hotshot::traits::implementations::Libp2pCommChannel;
use hotshot_types::data::ValidatingLeaf;
use hotshot_types::data::ValidatingProposal;
use hotshot_types::message::QuorumVote;
use hotshot_types::traits::node_implementation::NodeType;
use tracing::instrument;

pub mod infra;

use infra::main_entry_point;
use infra::CliOrchestrated;
use infra::Libp2pClientConfig;

type ThisLeaf = ValidatingLeaf<VDemoTypes>;
type ThisMembership =
    GeneralStaticCommittee<VDemoTypes, ThisLeaf, <VDemoTypes as NodeType>::SignatureKey>;
type ThisNetwork = Libp2pCommChannel<VDemoTypes, ThisProposal, ThisVote, ThisMembership>;
type ThisProposal = ValidatingProposal<VDemoTypes, ThisLeaf>;
type ThisVote = QuorumVote<VDemoTypes, ThisLeaf>;
type ThisNode = VDemoNode<ThisNetwork, ThisMembership>;
type ThisConfig = Libp2pClientConfig<VDemoTypes, ThisMembership>;

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
