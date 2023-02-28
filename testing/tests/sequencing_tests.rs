use ark_bls12_381::Parameters as Param381;
use either::Either::Right;
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
    },
};
use hotshot_testing::{test_description::GeneralTestDescriptionBuilder, TestNodeImpl};
use hotshot_types::{
    data::{DAProposal, SequencingLeaf, ViewNumber},
    traits::{node_implementation::NodeType, state::SequencingConsensus},
    vote::DAVote,
};
use jf_primitives::signatures::BLSSignatureScheme;
use tracing::instrument;

// Test the libp2p network with sequencing consensus.
#[instrument]
#[ignore]
fn description_build() -> GeneralTestDescriptionBuilder {
    GeneralTestDescriptionBuilder {
        round_start_delay: 25,
        num_bootstrap_nodes: 5,
        timeout_ratio: (11, 10),
        total_nodes: 10,
        start_nodes: 10,
        num_succeeds: 20,
        txn_ids: Right(1),
        next_view_timeout: 10000,
        start_delay: 120000,
        ..GeneralTestDescriptionBuilder::default()
    }
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
    type SignatureKey = JfPubKey<BLSSignatureScheme<Param381>>;
    type VoteTokenType = StaticVoteToken<JfPubKey<BLSSignatureScheme<Param381>>>;
    type Transaction = SDemoTransaction;
    type ElectionConfigType = StaticElectionConfig;
    type StateType = SDemoState;
}

// Test the memory network with sequencing consensus.
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn sequencing_memory_network_test() {
    let builder = description_build();

    builder
        .build::<SequencingTestTypes, TestNodeImpl<
            SequencingTestTypes,
            SequencingLeaf<SequencingTestTypes>,
            DAProposal<SequencingTestTypes>,
            DAVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
            MemoryCommChannel<
                SequencingTestTypes,
                DAProposal<SequencingTestTypes>,
                DAVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
                StaticCommittee<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
            >,
            MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
            StaticCommittee<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
        >>()
        .execute()
        .await
        .unwrap();
}

// Test the libp2p network with sequencing consensus.
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn sequencing_libp2p_test() {
    let builder = description_build();

    builder
        .build::<SequencingTestTypes, TestNodeImpl<
            SequencingTestTypes,
            SequencingLeaf<SequencingTestTypes>,
            DAProposal<SequencingTestTypes>,
            DAVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
            Libp2pCommChannel<
                SequencingTestTypes,
                DAProposal<SequencingTestTypes>,
                DAVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
                StaticCommittee<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
            >,
            MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
            StaticCommittee<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
        >>()
        .execute()
        .await
        .unwrap();
}

// Test the centralized server network with sequencing consensus.
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn sequencing_centralized_server_test() {
    let builder = description_build();

    builder
        .build::<SequencingTestTypes, TestNodeImpl<
            SequencingTestTypes,
            SequencingLeaf<SequencingTestTypes>,
            DAProposal<SequencingTestTypes>,
            DAVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
            CentralizedCommChannel<
                SequencingTestTypes,
                DAProposal<SequencingTestTypes>,
                DAVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
                StaticCommittee<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
            >,
            MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
            StaticCommittee<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
        >>()
        .execute()
        .await
        .unwrap();
}
