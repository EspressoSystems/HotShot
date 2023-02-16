use clap::Parser;
use hotshot::{
    demos::dentry::{DEntryNode, DEntryTypes},
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

type ThisLeaf = ValidatingLeaf<DEntryTypes>;
type ThisElection =
    GeneralStaticCommittee<DEntryTypes, ThisLeaf, <DEntryTypes as NodeType>::SignatureKey>;
type ThisNetwork = CentralizedCommChannel<DEntryTypes, ThisProposal, ThisVote, ThisElection>;
type ThisProposal = ValidatingProposal<DEntryTypes, ThisElection>;
type ThisVote = QuorumVote<DEntryTypes, ThisLeaf>;
type ThisNode = DEntryNode<ThisNetwork, ThisElection>;
type ThisConfig = CentralizedConfig<DEntryTypes, ThisElection>;

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
