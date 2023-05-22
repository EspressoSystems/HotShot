use async_compatibility_layer::logging::shutdown_logging;

use hotshot::traits::{
    election::static_committee::StaticCommittee,
    implementations::{MemoryStorage, WebCommChannel},
};

use hotshot_testing::{
    test_builder::{TestBuilder, TestMetadata, TimingData},
    test_types::StaticCommitteeTestTypes,
};
use hotshot_types::message::Message;
use hotshot_types::traits::{
    consensus_type::validating_consensus::ValidatingConsensus,
    election::QuorumExchange,
    node_implementation::{NodeImplementation, ValidatingExchanges},
};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    message::ValidatingMessage,
    vote::QuorumVote,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StaticCentralizedImp {}

type StaticMembership =
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>;

type StaticCommunication = WebCommChannel<
    ValidatingConsensus,
    StaticCommitteeTestTypes,
    StaticCentralizedImp,
    ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
>;

impl NodeImplementation<StaticCommitteeTestTypes> for StaticCentralizedImp {
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
            Message<StaticCommitteeTestTypes, Self>,
        >,
    >;
    type ConsensusMessage = ValidatingMessage<StaticCommitteeTestTypes, Self>;
}

/// Web server network test
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn web_server_network() {
    let builder = TestBuilder {
        metadata: TestMetadata {
            timing_data: TimingData {
                round_start_delay: 25,
                next_view_timeout: 3000,
                start_delay: 120000,
                ..Default::default()
            },
            num_succeeds: 5,
            ..TestMetadata::default()
        },
        ..Default::default()
    };

    builder
        .build::<StaticCommitteeTestTypes, StaticCentralizedImp>()
        .launch()
        .run_test()
        .await
        .unwrap();
    shutdown_logging();
}
