use hotshot::{
    traits::{
        election::static_committee::StaticCommittee,
        implementations::{MemoryStorage, WebServerWithFallbackCommChannel},
    },
    types::Message,
};
use hotshot_testing::{
    test_description::GeneralTestDescriptionBuilder, test_types::StaticCommitteeTestTypes,
};
use hotshot_types::traits::node_implementation::{NodeImplementation, ValidatingExchanges};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    vote::QuorumVote,
};
use hotshot_types::{message::ValidatingMessage, traits::election::QuorumExchange};
use serde::{Deserialize, Serialize};
use tracing::instrument;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FallbackImpl {}

type StaticMembership =
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>;

type StaticCommunication = WebServerWithFallbackCommChannel<
    StaticCommitteeTestTypes,
    FallbackImpl,
    ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
>;

impl NodeImplementation<StaticCommitteeTestTypes> for FallbackImpl {
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
            Message<StaticCommitteeTestTypes, FallbackImpl>,
        >,
    >;
    type ConsensusMessage = ValidatingMessage<StaticCommitteeTestTypes, Self>;
}

/// web server with libp2p network test
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn webserver_libp2p_network() {
    let description = GeneralTestDescriptionBuilder::default_multiple_rounds();

    description
        .build::<StaticCommitteeTestTypes, FallbackImpl>()
        .execute()
        .await
        .unwrap();
}

// stress test for web server with libp2p
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_stress_webserver_libp2p_network() {
    let description = GeneralTestDescriptionBuilder::default_stress();

    description
        .build::<StaticCommitteeTestTypes, FallbackImpl>()
        .execute()
        .await
        .unwrap();
}
