#![allow(dead_code)]

use crate::{test_description::VrfTestMetaData, TestNodeImpl, TestRunner};
use ark_bls12_381::Parameters as Param381;
use blake3::Hasher;
use hotshot::{
    demos::vdemo::{VDemoBlock, VDemoState, VDemoTransaction},
    traits::{
        dummy::DummyState,
        election::{
            static_committee::{StaticCommittee, StaticElectionConfig, StaticVoteToken},
            vrf::{JfPubKey, VRFStakeTableConfig, VRFVoteToken, VrfImpl},
        },
        implementations::{MemoryCommChannel, MemoryStorage},
    },
};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal, ViewNumber},
    traits::{
        block_contents::dummy::{DummyBlock, DummyTransaction},
        node_implementation::{ApplicationMetadata, NodeType},
        state::ValidatingConsensus,
    },
    vote::QuorumVote,
};
use jf_primitives::{
    signatures::{
        bls::{BLSSignature, BLSVerKey},
        BLSSignatureScheme,
    },
    vrf::blsvrf::BLSVRFScheme,
};
use serde::{Deserialize, Serialize};

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
/// vrf test types
pub struct VrfTestTypes;
impl NodeType for VrfTestTypes {
    // TODO (da) can this be SequencingConsensus?
    type ConsensusType = ValidatingConsensus;
    type Time = ViewNumber;
    type BlockType = DummyBlock;
    type SignatureKey = JfPubKey<BLSSignatureScheme<Param381>>;
    type VoteTokenType = VRFVoteToken<BLSVerKey<Param381>, BLSSignature<Param381>>;
    type Transaction = DummyTransaction;
    type ElectionConfigType = VRFStakeTableConfig;
    type StateType = DummyState;
    type ApplicationMetadataType = VrfTestMetaData;
}

/// application metadata stub
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct StaticCommitteeMetaData {}

impl ApplicationMetadata for StaticCommitteeMetaData {}

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
/// static committee test types
pub struct StaticCommitteeTestTypes;
impl NodeType for StaticCommitteeTestTypes {
    type ConsensusType = ValidatingConsensus;
    type Time = ViewNumber;
    type BlockType = VDemoBlock;
    type SignatureKey = JfPubKey<BLSSignatureScheme<Param381>>;
    type VoteTokenType = StaticVoteToken<JfPubKey<BLSSignatureScheme<Param381>>>;
    type Transaction = VDemoTransaction;
    type ElectionConfigType = StaticElectionConfig;
    type StateType = VDemoState;
    type ApplicationMetadataType = StaticCommitteeMetaData;
}

/// type synonym for vrf committee election
/// with in-memory network
pub type StandardNodeImplType = TestNodeImpl<
    VrfTestTypes,
    ValidatingLeaf<VrfTestTypes>,
    ValidatingProposal<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
    QuorumVote<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
    MemoryCommChannel<
        VrfTestTypes,
        ValidatingProposal<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
        QuorumVote<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
        VrfImpl<
            VrfTestTypes,
            ValidatingLeaf<VrfTestTypes>,
            BLSSignatureScheme<Param381>,
            BLSVRFScheme<Param381>,
            Hasher,
            Param381,
        >,
    >,
    MemoryStorage<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
    VrfImpl<
        VrfTestTypes,
        ValidatingLeaf<VrfTestTypes>,
        BLSSignatureScheme<Param381>,
        BLSVRFScheme<Param381>,
        Hasher,
        Param381,
    >,
>;

/// type synonym for static committee
/// with in-memory network
pub type StaticNodeImplType = TestNodeImpl<
    StaticCommitteeTestTypes,
    ValidatingLeaf<StaticCommitteeTestTypes>,
    ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    MemoryCommChannel<
        StaticCommitteeTestTypes,
        ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
        QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
        StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    >,
    MemoryStorage<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
>;

/// type alias for the test runner type
pub type AppliedTestRunner<TYPES, LEAF, PROPOSAL, VOTE, MEMBERSHIP> =
    TestRunner<TYPES, AppliedTestNodeImpl<TYPES, LEAF, PROPOSAL, VOTE, MEMBERSHIP>>;
/// applied test runner (convenient type alias)
pub type AppliedTestNodeImpl<TYPES, LEAF, PROPOSAL, VOTE, MEMBERSHIP> = TestNodeImpl<
    TYPES,
    LEAF,
    PROPOSAL,
    VOTE,
    MemoryCommChannel<TYPES, PROPOSAL, VOTE, MEMBERSHIP>,
    MemoryStorage<TYPES, LEAF>,
    MEMBERSHIP,
>;
