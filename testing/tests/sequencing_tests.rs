// Sishan NOTE TODO: Whether we should comment these for easier testing
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
use jf_primitives::signatures::bls_over_bn254::{BLSOverBN254CurveSignatureScheme, KeyPair};
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
    type SignatureKey = JfPubKey<BLSOverBN254CurveSignatureScheme>;
    type VoteTokenType = StaticVoteToken<Self::SignatureKey>;
    type Transaction = SDemoTransaction;
    type ElectionConfigType = StaticElectionConfig;
    type StateType = SDemoState;
}

// #[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
// pub struct SequencingMemoryImpl {}

// type StaticMembership = StaticCommittee<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;

// type StaticDAComm = MemoryCommChannel<
//     SequencingTestTypes,
//     SequencingMemoryImpl,
//     DAProposal<SequencingTestTypes>,
//     DAVote<SequencingTestTypes>,
//     StaticMembership,
// >;

// type StaticQuroumComm = MemoryCommChannel<
//     SequencingTestTypes,
//     SequencingMemoryImpl,
//     QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     StaticMembership,
// >;

// type StaticViewSyncComm = MemoryCommChannel<
//     SequencingTestTypes,
//     SequencingMemoryImpl,
//     QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     ViewSyncVote<SequencingTestTypes>,
//     StaticMembership,
// >;

// impl NodeImplementation<SequencingTestTypes> for SequencingMemoryImpl {
//     type Storage = MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;
//     type Leaf = SequencingLeaf<SequencingTestTypes>;
//     type Exchanges = SequencingExchanges<
//         SequencingTestTypes,
//         Message<SequencingTestTypes, Self>,
//         QuorumExchange<
//             SequencingTestTypes,
//             Self::Leaf,
//             QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//             StaticMembership,
//             StaticQuroumComm,
//             Message<SequencingTestTypes, Self>,
//         >,
//         CommitteeExchange<
//             SequencingTestTypes,
//             StaticMembership,
//             StaticDAComm,
//             Message<SequencingTestTypes, Self>,
//         >,
//         ViewSyncExchange<
//             SequencingTestTypes,
//             QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//             StaticMembership,
//             StaticViewSyncComm,
//             Message<SequencingTestTypes, Self>,
//         >,
//     >;
//     type ConsensusMessage = SequencingMessage<SequencingTestTypes, Self>;

//     fn new_channel_maps(
//         start_view: ViewNumber,
//     ) -> (
//         ChannelMaps<SequencingTestTypes, Self>,
//         Option<ChannelMaps<SequencingTestTypes, Self>>,
//     ) {
//         (
//             ChannelMaps::new(start_view),
//             Some(ChannelMaps::new(start_view)),
//         )
//     }
// }

// // Test the memory network with sequencing consensus.
// #[cfg_attr(
//     feature = "tokio-executor",
//     tokio::test(flavor = "multi_thread", worker_threads = 2)
// )]
// #[cfg_attr(feature = "async-std-executor", async_std::test)]
// #[instrument]
// async fn sequencing_memory_network_test() {
//     let builder: TestBuilder = TestBuilder::default_multiple_rounds_da();

//     builder
//         .build::<SequencingTestTypes, SequencingMemoryImpl>()
//         .launch()
//         .run_test()
//         .await
//         .unwrap();
// }

// #[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
// struct SequencingLibP2PImpl {}

// type StaticDACommP2p = Libp2pCommChannel<
//     SequencingTestTypes,
//     SequencingLibP2PImpl,
//     DAProposal<SequencingTestTypes>,
//     DAVote<SequencingTestTypes>,
//     StaticMembership,
// >;

// type StaticQuroumCommP2p = Libp2pCommChannel<
//     SequencingTestTypes,
//     SequencingLibP2PImpl,
//     QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     StaticMembership,
// >;

// type StaticViewSyncCommP2P = Libp2pCommChannel<
//     SequencingTestTypes,
//     SequencingLibP2PImpl,
//     QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     ViewSyncVote<SequencingTestTypes>,
//     StaticMembership,
// >;

// impl NodeImplementation<SequencingTestTypes> for SequencingLibP2PImpl {
//     type Storage = MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;
//     type Leaf = SequencingLeaf<SequencingTestTypes>;
//     type Exchanges = SequencingExchanges<
//         SequencingTestTypes,
//         Message<SequencingTestTypes, Self>,
//         QuorumExchange<
//             SequencingTestTypes,
//             Self::Leaf,
//             QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//             StaticMembership,
//             StaticQuroumCommP2p,
//             Message<SequencingTestTypes, Self>,
//         >,
//         CommitteeExchange<
//             SequencingTestTypes,
//             StaticMembership,
//             StaticDACommP2p,
//             Message<SequencingTestTypes, Self>,
//         >,
//         ViewSyncExchange<
//             SequencingTestTypes,
//             QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//             StaticMembership,
//             StaticViewSyncCommP2P,
//             Message<SequencingTestTypes, Self>,
//         >,
//     >;
//     type ConsensusMessage = SequencingMessage<SequencingTestTypes, Self>;

//     fn new_channel_maps(
//         start_view: ViewNumber,
//     ) -> (
//         ChannelMaps<SequencingTestTypes, Self>,
//         Option<ChannelMaps<SequencingTestTypes, Self>>,
//     ) {
//         (
//             ChannelMaps::new(start_view),
//             Some(ChannelMaps::new(start_view)),
//         )
//     }
// }

// // Test the libp2p network with sequencing consensus.
// #[cfg_attr(
//     feature = "tokio-executor",
//     tokio::test(flavor = "multi_thread", worker_threads = 2)
// )]
// #[cfg_attr(feature = "async-std-executor", async_std::test)]
// #[instrument]
// async fn sequencing_libp2p_test() {
//     let builder = TestBuilder::default_multiple_rounds_da();

//     builder
//         .build::<SequencingTestTypes, SequencingLibP2PImpl>()
//         .launch()
//         .run_test()
//         .await
//         .unwrap();
// }

// #[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
// struct SequencingCentralImpl {}

// type StaticDACommCentral = CentralizedCommChannel<
//     SequencingTestTypes,
//     SequencingCentralImpl,
//     DAProposal<SequencingTestTypes>,
//     DAVote<SequencingTestTypes>,
//     StaticMembership,
// >;

// type StaticQuroumCommCentral = CentralizedCommChannel<
//     SequencingTestTypes,
//     SequencingCentralImpl,
//     QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     StaticMembership,
// >;

// type StaticViewSyncCommCentral = CentralizedCommChannel<
//     SequencingTestTypes,
//     SequencingCentralImpl,
//     QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     ViewSyncVote<SequencingTestTypes>,
//     StaticMembership,
// >;

// impl NodeImplementation<SequencingTestTypes> for SequencingCentralImpl {
//     type Storage = MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;
//     type Leaf = SequencingLeaf<SequencingTestTypes>;
//     type Exchanges = SequencingExchanges<
//         SequencingTestTypes,
//         Message<SequencingTestTypes, Self>,
//         QuorumExchange<
//             SequencingTestTypes,
//             Self::Leaf,
//             QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//             StaticMembership,
//             StaticQuroumCommCentral,
//             Message<SequencingTestTypes, Self>,
//         >,
//         CommitteeExchange<
//             SequencingTestTypes,
//             StaticMembership,
//             StaticDACommCentral,
//             Message<SequencingTestTypes, Self>,
//         >,
//         ViewSyncExchange<
//             SequencingTestTypes,
//             QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//             StaticMembership,
//             StaticViewSyncCommCentral,
//             Message<SequencingTestTypes, Self>,
//         >,
//     >;
//     type ConsensusMessage = SequencingMessage<SequencingTestTypes, Self>;

//     fn new_channel_maps(
//         start_view: ViewNumber,
//     ) -> (
//         ChannelMaps<SequencingTestTypes, Self>,
//         Option<ChannelMaps<SequencingTestTypes, Self>>,
//     ) {
//         (
//             ChannelMaps::new(start_view),
//             Some(ChannelMaps::new(start_view)),
//         )
//     }
// }

// // Test the centralized server network with sequencing consensus.
// #[cfg_attr(
//     feature = "tokio-executor",
//     tokio::test(flavor = "multi_thread", worker_threads = 2)
// )]
// #[cfg_attr(feature = "async-std-executor", async_std::test)]
// #[instrument]
// async fn sequencing_centralized_server_test() {
//     let builder = TestBuilder::default_multiple_rounds_da();

//     builder
//         .build::<SequencingTestTypes, SequencingCentralImpl>()
//         .launch()
//         .run_test()
//         .await
//         .unwrap();
// }
