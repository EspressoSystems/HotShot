use ark_bls12_381::Parameters as Param381;
use hotshot::{
    demos::sdemo::{SDemoBlock, SDemoState, SDemoTransaction},
    traits::{
        election::{
            static_committee::{StaticCommittee, StaticElectionConfig, StaticVoteToken},
            vrf::JfPubKey,
        },
        implementations::{
            CentralizedCommChannel, Libp2pCommChannel, MemoryCommChannel, MemoryStorage,
        },
        NodeImplementation,
    },
};
use hotshot_testing::test_builder::TestBuilder;
use hotshot_types::data::QuorumProposal;
use hotshot_types::message::{Message, SequencingMessage};
use hotshot_types::vote::QuorumVote;
use hotshot_types::{
    data::{DAProposal, SequencingLeaf, ViewNumber},
    traits::{
        consensus_type::sequencing_consensus::SequencingConsensus,
        election::{CommitteeExchange, QuorumExchange},
        node_implementation::{NodeType, SequencingExchanges},
    },
    vote::DAVote,
};
use jf_primitives::signatures::BLSSignatureScheme;
use serde::{Deserialize, Serialize};
use tracing::instrument;

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
    type SignatureKey = JfPubKey<BLSSignatureScheme<Param381>>;
    type VoteTokenType = StaticVoteToken<Self::SignatureKey>;
    type Transaction = SDemoTransaction;
    type ElectionConfigType = StaticElectionConfig;
    type StateType = SDemoState;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SequencingMemoryImpl {}

type StaticMembership = StaticCommittee<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;

type StaticDAComm = MemoryCommChannel<
    SequencingTestTypes,
    SequencingMemoryImpl,
    DAProposal<SequencingTestTypes>,
    DAVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    StaticMembership,
>;

type StaticQuroumComm = MemoryCommChannel<
    SequencingTestTypes,
    SequencingMemoryImpl,
    QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    StaticMembership,
>;

impl NodeImplementation<SequencingTestTypes> for SequencingMemoryImpl {
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

// Test the memory network with sequencing consensus.
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn sequencing_memory_network_test() {
    let builder: TestBuilder = TestBuilder::default_multiple_rounds_da();

    builder
        .build::<SequencingTestTypes, SequencingMemoryImpl>()
        .launch()
        .run_test()
        .await
        .unwrap();
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SequencingLibP2PImpl {}

type StaticDACommP2p = Libp2pCommChannel<
    SequencingTestTypes,
    SequencingLibP2PImpl,
    DAProposal<SequencingTestTypes>,
    DAVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    StaticMembership,
>;

type StaticQuroumCommP2p = Libp2pCommChannel<
    SequencingTestTypes,
    SequencingLibP2PImpl,
    QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    StaticMembership,
>;

impl NodeImplementation<SequencingTestTypes> for SequencingLibP2PImpl {
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
            StaticQuroumCommP2p,
            Message<SequencingTestTypes, Self>,
        >,
        CommitteeExchange<
            SequencingTestTypes,
            StaticMembership,
            StaticDACommP2p,
            Message<SequencingTestTypes, Self>,
        >,
    >;
    type ConsensusMessage = SequencingMessage<SequencingTestTypes, Self>;
}

// Test the libp2p network with sequencing consensus.
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn sequencing_libp2p_test() {
    let builder = TestBuilder::default_multiple_rounds_da();

    builder
        .build::<SequencingTestTypes, SequencingLibP2PImpl>()
        .launch()
        .run_test()
        .await
        .unwrap();
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SequencingCentralImpl {}

type StaticDACommCentral = CentralizedCommChannel<
    SequencingTestTypes,
    SequencingCentralImpl,
    DAProposal<SequencingTestTypes>,
    DAVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    StaticMembership,
>;

type StaticQuroumCommCentral = CentralizedCommChannel<
    SequencingTestTypes,
    SequencingCentralImpl,
    QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    StaticMembership,
>;

impl NodeImplementation<SequencingTestTypes> for SequencingCentralImpl {
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
            StaticQuroumCommCentral,
            Message<SequencingTestTypes, Self>,
        >,
        CommitteeExchange<
            SequencingTestTypes,
            StaticMembership,
            StaticDACommCentral,
            Message<SequencingTestTypes, Self>,
        >,
    >;
    type ConsensusMessage = SequencingMessage<SequencingTestTypes, Self>;
}

// Test the centralized server network with sequencing consensus.
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn sequencing_centralized_server_test() {
    let builder = TestBuilder::default_multiple_rounds_da();

    builder
        .build::<SequencingTestTypes, SequencingCentralImpl>()
        .launch()
        .run_test()
        .await
        .unwrap();
}
