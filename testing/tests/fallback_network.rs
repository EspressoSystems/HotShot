use hotshot::{
    traits::{
        election::static_committee::StaticCommittee,
        implementations::{MemoryStorage, WebServerWithFallbackCommChannel},
    },
    types::Message,
};
use hotshot_testing::{
    test_types::StaticCommitteeTestTypes, test_builder::TestBuilder,
};
use hotshot_types::traits::election::QuorumExchange;

use hotshot_types::traits::node_implementation::NodeImplementation;
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    vote::QuorumVote,
};
use tracing::instrument;

#[derive(Clone, Debug)]
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
    type QuorumExchange = QuorumExchange<
        StaticCommitteeTestTypes,
        ValidatingLeaf<StaticCommitteeTestTypes>,
        ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
        StaticMembership,
        StaticCommunication,
        Message<StaticCommitteeTestTypes, FallbackImpl>,
    >;
    type CommitteeExchange = Self::QuorumExchange;
}

/// web server with libp2p network test
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn webserver_libp2p_network() {
    let builder = TestBuilder::<StaticCommitteeTestTypes, FallbackImpl>::default_multiple_rounds();

    builder.build().launch().run_test().await.unwrap();

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
    let builder = TestBuilder::<StaticCommitteeTestTypes, FallbackImpl>::default_multiple_rounds();

    builder.build().launch().run_test().await.unwrap();
}
