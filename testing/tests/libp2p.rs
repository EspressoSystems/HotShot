use hotshot::{
    traits::{
        election::static_committee::StaticCommittee,
        implementations::{Libp2pCommChannel, MemoryStorage},
    },
    types::Message,
};
use hotshot_testing::{test_builder::TestBuilder, test_types::StaticCommitteeTestTypes};
use hotshot_types::traits::election::QuorumExchange;

use hotshot_types::traits::node_implementation::{NodeImplementation, ValidatingExchanges};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    message::ValidatingMessage,
    vote::QuorumVote,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Libp2pImpl {}

type StaticMembership =
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>;

type StaticCommunication = Libp2pCommChannel<
    StaticCommitteeTestTypes,
    Libp2pImpl,
    ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
>;

impl NodeImplementation<StaticCommitteeTestTypes> for Libp2pImpl {
    type Storage =
        MemoryStorage<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>;
    type Leaf = ValidatingLeaf<StaticCommitteeTestTypes>;
    type Exchanges = ValidatingExchanges<
        StaticCommitteeTestTypes,
        Message<StaticCommitteeTestTypes, Self>,
        QuorumExchange<
            StaticCommitteeTestTypes,
            ValidatingLeaf<StaticCommitteeTestTypes>,
            ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            StaticMembership,
            StaticCommunication,
            Message<StaticCommitteeTestTypes, Libp2pImpl>,
        >,
    >;
    type ConsensusMessage = ValidatingMessage<StaticCommitteeTestTypes, Self>;
}

/// libp2p network test
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn libp2p_network() {
    let builder = TestBuilder::default_multiple_rounds();

    builder
        .build::<StaticCommitteeTestTypes, Libp2pImpl>()
        .launch()
        .run_test()
        .await
        .unwrap();
}

// stress test for libp2p
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_stress_libp2p_network() {
    let builder = TestBuilder::default_stress();
    builder
        .build::<StaticCommitteeTestTypes, Libp2pImpl>()
        .launch()
        .run_test()
        .await
        .unwrap();
}
