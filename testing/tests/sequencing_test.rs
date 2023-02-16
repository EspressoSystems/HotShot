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
    data::{ValidatingLeaf, ValidatingProposal, ViewNumber},
    traits::{node_implementation::NodeType, state::ValidatingConsensus}, message::QuorumVote,
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
    type ConsensusType = ValidatingConsensus;
    type Time = ViewNumber;
    type BlockType = SDemoBlock;
    type SignatureKey = JfPubKey<BLSSignatureScheme<Param381>>;
    type VoteTokenType = StaticVoteToken<JfPubKey<BLSSignatureScheme<Param381>>>;
    type Transaction = SDemoTransaction;
    type ElectionConfigType = StaticElectionConfig;
    type StateType = SDemoState;
    type ApplicationMetadataType = StaticCommitteeMetaData;
}

// stress test for libp2p
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn sequencing_test() {
    let description = GeneralTestDescriptionBuilder {
        round_start_delay: 25,
        num_bootstrap_nodes: 15,
        timeout_ratio: (1, 1),
        total_nodes: 100,
        start_nodes: 100,
        num_succeeds: 5,
        txn_ids: Right(1),
        next_view_timeout: 2000,
        start_delay: 20000,
        ..GeneralTestDescriptionBuilder::default()
    };

    description
        .build::<SequencingTestTypes, TestNodeImpl<
            SequencingTestTypes,
            ValidatingLeaf<SequencingTestTypes>,
            ValidatingProposal<
                SequencingTestTypes,
                StaticCommittee<SequencingTestTypes, ValidatingLeaf<SequencingTestTypes>>,
            >,
            QuorumVote<SequencingTestTypes, ValidatingLeaf<SequencingTestTypes>>,
            MemoryCommChannel<
                SequencingTestTypes,
                ValidatingProposal<
                    SequencingTestTypes,
                    StaticCommittee<SequencingTestTypes, ValidatingLeaf<SequencingTestTypes>>,
                >,
                QuorumVote<SequencingTestTypes, ValidatingLeaf<SequencingTestTypes>>,
                StaticCommittee<SequencingTestTypes, ValidatingLeaf<SequencingTestTypes>>,
            >,
            MemoryStorage<SequencingTestTypes, ValidatingLeaf<SequencingTestTypes>>,
            StaticCommittee<SequencingTestTypes, ValidatingLeaf<SequencingTestTypes>>,
        >>()
        .execute()
        .await
        .unwrap();
}
