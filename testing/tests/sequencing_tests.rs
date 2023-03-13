// use ark_bls12_381::Parameters as Param381;
// use hotshot::{
//     demos::sdemo::{SDemoBlock, SDemoState, SDemoTransaction},
//     traits::{
//         election::{
//             static_committee::{StaticCommittee, StaticElectionConfig, StaticVoteToken},
//             vrf::JfPubKey,
//         },
//         implementations::{
//             CentralizedCommChannel, Libp2pCommChannel, MemoryCommChannel, MemoryStorage,
//         },
//         NodeImplementation,
//     },
// };
// use hotshot_testing::test_description::GeneralTestDescriptionBuilder;
// use hotshot_types::data::CommitmentProposal;
// use hotshot_types::message::Message;
// use hotshot_types::traits::election::ConsensusExchange;
// use hotshot_types::traits::node_implementation::{
//     QuorumMembership, QuorumProposal, QuorumVoteType,
// };
// use hotshot_types::traits::storage::TestableStorage;
// use hotshot_types::vote::QuorumVote;
// use hotshot_types::{
//     data::{DAProposal, SequencingLeaf, ViewNumber},
//     traits::{
//         election::{CommitteeExchange, QuorumExchange},
//         node_implementation::NodeType,
//         state::SequencingConsensus,
//     },
//     vote::DAVote,
// };
// use jf_primitives::signatures::BLSSignatureScheme;
// use tracing::instrument;
//
// #[derive(
//     Copy,
//     Clone,
//     Debug,
//     Default,
//     Hash,
//     PartialEq,
//     Eq,
//     PartialOrd,
//     Ord,
//     serde::Serialize,
//     serde::Deserialize,
// )]
// pub struct SequencingTestTypes;
// impl NodeType for SequencingTestTypes {
//     type ConsensusType = SequencingConsensus;
//     type Time = ViewNumber;
//     type BlockType = SDemoBlock;
//     type SignatureKey = JfPubKey<BLSSignatureScheme<Param381>>;
//     type VoteTokenType = StaticVoteToken<Self::SignatureKey>;
//     type Transaction = SDemoTransaction;
//     type ElectionConfigType = StaticElectionConfig;
//     type StateType = SDemoState;
// }
//
// #[derive(Clone, Debug)]
// struct SequencingMemoryImpl {}
//
// type StaticMembership = StaticCommittee<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;
//
// type StaticDAComm = MemoryCommChannel<
//     SequencingTestTypes,
//     SequencingMemoryImpl,
//     DAProposal<SequencingTestTypes>,
//     DAVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     StaticMembership,
// >;
//
// type StaticQuroumComm = MemoryCommChannel<
//     SequencingTestTypes,
//     SequencingMemoryImpl,
//     CommitmentProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     StaticMembership,
// >;
//
// impl NodeImplementation<SequencingTestTypes> for SequencingMemoryImpl {
//     type Storage = MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;
//     type Leaf = SequencingLeaf<SequencingTestTypes>;
//     type QuorumExchange = QuorumExchange<
//         SequencingTestTypes,
//         Self::Leaf,
//         CommitmentProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//         StaticMembership,
//         StaticQuroumComm,
//         Message<SequencingTestTypes, Self>,
//     >;
//     type CommitteeExchange = CommitteeExchange<
//         SequencingTestTypes,
//         Self::Leaf,
//         StaticMembership,
//         StaticDAComm,
//         Message<SequencingTestTypes, Self>,
//     >;
// }
//
// // Test the memory network with sequencing consensus.
// #[cfg_attr(
//     feature = "tokio-executor",
//     tokio::test(flavor = "multi_thread", worker_threads = 2)
// )]
// #[cfg_attr(feature = "async-std-executor", async_std::test)]
// #[instrument]
// async fn sequencing_memory_network_test() {
//     let builder = GeneralTestDescriptionBuilder::default_multiple_rounds();
//
//     builder
//         .build::<SequencingTestTypes, SequencingMemoryImpl>()
//         .execute()
//         .await
//         .unwrap();
// }
//
// #[derive(Clone, Debug)]
// struct SequencingLibP2PImpl {}
//
// type StaticDACommP2p = Libp2pCommChannel<
//     SequencingTestTypes,
//     SequencingLibP2PImpl,
//     DAProposal<SequencingTestTypes>,
//     DAVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     StaticMembership,
// >;
//
// type StaticQuroumCommP2p = Libp2pCommChannel<
//     SequencingTestTypes,
//     SequencingLibP2PImpl,
//     CommitmentProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     StaticMembership,
// >;
//
// impl NodeImplementation<SequencingTestTypes> for SequencingLibP2PImpl {
//     type Storage = MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;
//     type Leaf = SequencingLeaf<SequencingTestTypes>;
//     type QuorumExchange = QuorumExchange<
//         SequencingTestTypes,
//         Self::Leaf,
//         CommitmentProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//         StaticMembership,
//         StaticQuroumCommP2p,
//         Message<SequencingTestTypes, Self>,
//     >;
//     type CommitteeExchange = CommitteeExchange<
//         SequencingTestTypes,
//         Self::Leaf,
//         StaticMembership,
//         StaticDACommP2p,
//         Message<SequencingTestTypes, Self>,
//     >;
// }
//
// // Test the libp2p network with sequencing consensus.
// #[cfg_attr(
//     feature = "tokio-executor",
//     tokio::test(flavor = "multi_thread", worker_threads = 2)
// )]
// #[cfg_attr(feature = "async-std-executor", async_std::test)]
// #[instrument]
// #[ignore]
// async fn sequencing_libp2p_test() {
//     let builder = GeneralTestDescriptionBuilder::default_multiple_rounds();
//
//     builder
//         .build::<SequencingTestTypes, SequencingLibP2PImpl>()
//         .execute()
//         .await
//         .unwrap();
// }
//
// #[derive(Clone, Debug)]
// struct SequencingCentralImpl {}
//
// type StaticDACommCentral = CentralizedCommChannel<
//     SequencingTestTypes,
//     SequencingCentralImpl,
//     DAProposal<SequencingTestTypes>,
//     DAVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     StaticMembership,
// >;
//
// type StaticQuroumCommCentral = Libp2pCommChannel<
//     SequencingTestTypes,
//     SequencingCentralImpl,
//     CommitmentProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//     StaticMembership,
// >;
//
// impl NodeImplementation<SequencingTestTypes> for SequencingCentralImpl {
//     type Storage = MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;
//     type Leaf = SequencingLeaf<SequencingTestTypes>;
//     type QuorumExchange = QuorumExchange<
//         SequencingTestTypes,
//         Self::Leaf,
//         CommitmentProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
//         StaticMembership,
//         StaticQuroumCommCentral,
//         Message<SequencingTestTypes, Self>,
//     >;
//     type CommitteeExchange = CommitteeExchange<
//         SequencingTestTypes,
//         Self::Leaf,
//         StaticMembership,
//         StaticDACommCentral,
//         Message<SequencingTestTypes, Self>,
//     >;
// }
//
// // Test the centralized server network with sequencing consensus.
// #[cfg_attr(
//     feature = "tokio-executor",
//     tokio::test(flavor = "multi_thread", worker_threads = 2)
// )]
// #[cfg_attr(feature = "async-std-executor", async_std::test)]
// #[instrument]
// #[ignore]
// async fn sequencing_centralized_server_test() {
//     let builder = GeneralTestDescriptionBuilder::default_multiple_rounds();
//
//     builder
//         .build::<SequencingTestTypes, SequencingCentralImpl>()
//         .execute()
//         .await
//         .unwrap();
// }
