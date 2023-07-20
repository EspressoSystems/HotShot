use async_compatibility_layer::logging::shutdown_logging;
use hotshot::demos::sdemo::SDemoState;
use hotshot::demos::sdemo::SDemoTransaction;
use hotshot::traits::election::static_committee::StaticElectionConfig;
use hotshot::traits::election::static_committee::StaticVoteToken;
use hotshot::traits::election::vrf::JfPubKey;
use hotshot_types::data::DAProposal;
use hotshot_types::data::QuorumProposal;
use hotshot_types::data::SequencingLeaf;
use hotshot_types::message::SequencingMessage;
use hotshot_types::traits::consensus_type::sequencing_consensus::SequencingConsensus;
use hotshot_types::traits::election::CommitteeExchange;
use hotshot_types::traits::node_implementation::NodeType;
use hotshot_types::traits::node_implementation::SequencingExchanges;
use hotshot_types::vote::DAVote;

// Sishan NOTE: for QC aggregation
use jf_primitives::signatures::bls_over_bn254::{BLSOverBN254CurveSignatureScheme};

use hotshot::demos::sdemo::SDemoBlock;
use hotshot_types::data::ViewNumber;

use hotshot::traits::{
    election::static_committee::StaticCommittee,
    implementations::{MemoryStorage, WebCommChannel},
};

use hotshot_testing::{test_builder::TestBuilder, test_types::StaticCommitteeTestTypes};
use hotshot_types::message::Message;
use hotshot_types::traits::{
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
use tracing::{warn};

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StaticCentralizedImp {}

type StaticMembershipV =
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>;

type StaticCommunication = WebCommChannel<
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
            StaticMembershipV,
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
    let builder: TestBuilder = TestBuilder::default_multiple_rounds();

    builder
        .build::<StaticCommitteeTestTypes, StaticCentralizedImp>()
        .launch()
        .run_test()
        .await
        .unwrap();
    shutdown_logging();
}

#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct SequencingTestTypes;
impl NodeType for SequencingTestTypes {
    type ConsensusType = SequencingConsensus;
    type Time = ViewNumber;
    type BlockType = SDemoBlock;
    type SignatureKey = JfPubKey<BLSOverBN254CurveSignatureScheme>; // Sishan NOTE: for QC aggregation
    type VoteTokenType = StaticVoteToken<Self::SignatureKey>;
    type Transaction = SDemoTransaction;
    type ElectionConfigType = StaticElectionConfig;
    type StateType = SDemoState;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SequencingWebServerImpl {}

type StaticMembership = StaticCommittee<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;

type StaticDAComm = WebCommChannel<
    SequencingTestTypes,
    SequencingWebServerImpl,
    DAProposal<SequencingTestTypes>,
    DAVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    StaticMembership,
>;

type StaticQuroumComm = WebCommChannel<
    SequencingTestTypes,
    SequencingWebServerImpl,
    QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    StaticMembership,
>;

impl NodeImplementation<SequencingTestTypes> for SequencingWebServerImpl {
    type Storage = MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;
    type Leaf = SequencingLeaf<SequencingTestTypes>;
    type Exchanges = SequencingExchanges<
        SequencingTestTypes,
        Message<SequencingTestTypes, Self>,
        QuorumExchange<
            SequencingTestTypes,
            Self::Leaf,
            QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
            StaticMembership,
            StaticQuroumComm,
            Message<SequencingTestTypes, Self>,
        >,
        CommitteeExchange<
            SequencingTestTypes,
            StaticMembership,
            StaticDAComm,
            Message<SequencingTestTypes, Self>,
        >,
    >;
    type ConsensusMessage = SequencingMessage<SequencingTestTypes, Self>;
}

// Test the web server with sequencing consensus
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn sequencing_web_server_test() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();

    let builder: TestBuilder = TestBuilder::default_multiple_rounds_da();
    builder
        .build::<SequencingTestTypes, SequencingWebServerImpl>()
        .launch()
        .run_test()
        .await
        .unwrap();
}
