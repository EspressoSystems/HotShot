mod common;
use ark_bls12_381::Parameters as Param381;
use common::*;
use either::Either::Right;
use hotshot::{
    demos::sdemo::{SDemoBlock, SDemoState, SDemoTransaction},
    traits::{
        election::{
            static_committee::{StaticCommittee, StaticElectionConfig, StaticVoteToken},
            vrf::JfPubKey,
        },
        implementations::{MemoryCommChannel, MemoryStorage},
    },
};
use hotshot_testing::TestNodeImpl;
use hotshot_types::{
    data::{DAProposal, SequencingLeaf, ViewNumber},
    traits::{node_implementation::NodeType, state::SequencingConsensus},
    vote::DAVote,
};
use jf_primitives::signatures::BLSSignatureScheme;
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
    type VoteTokenType = StaticVoteToken<JfPubKey<BLSSignatureScheme<Param381>>>;
    type Transaction = SDemoTransaction;
    type ElectionConfigType = StaticElectionConfig;
    type StateType = SDemoState;
    type ApplicationMetadataType = StaticCommitteeMetaData;
}

// Test the memory network with sequencing consensus.
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn sequencing_memory_test() {
    let description = GeneralTestDescriptionBuilder {
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
    };

    description
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
