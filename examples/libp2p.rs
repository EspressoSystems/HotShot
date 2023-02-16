use clap::Parser;
use hotshot::demos::dentry::DEntryNode;
use hotshot::demos::dentry::DEntryTypes;
use hotshot::traits::election::static_committee::GeneralStaticCommittee;
use hotshot::traits::implementations::Libp2pCommChannel;
use hotshot_types::data::ValidatingLeaf;
use hotshot_types::data::ValidatingProposal;
use hotshot_types::traits::node_implementation::NodeType;
use tracing::instrument;

pub mod infra;

use infra::main_entry_point;
use infra::CliOrchestrated;
use infra::Libp2pClientConfig;

type ThisLeaf = ValidatingLeaf<DEntryTypes>;
type ThisElection =
    GeneralStaticCommittee<DEntryTypes, ThisLeaf, <DEntryTypes as NodeType>::SignatureKey>;
type ThisNetwork = Libp2pCommChannel<DEntryTypes, ThisLeaf, ThisProposal, ThisElection>;
type ThisProposal = ValidatingProposal<DEntryTypes, ThisLeaf>;
type ThisNode = DEntryNode<ThisNetwork, ThisElection>;
type ThisConfig = Libp2pClientConfig<DEntryTypes, ThisElection>;

#[cfg_attr(
    feature = "tokio-executor",
    tokio::main(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::main)]
#[instrument]
async fn main() {
    let args = CliOrchestrated::parse();

    main_entry_point::<DEntryTypes, ThisElection, ThisNetwork, ThisNode, ThisConfig>(args).await;
}
