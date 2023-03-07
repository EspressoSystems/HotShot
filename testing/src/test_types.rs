#![allow(dead_code)]

use crate::{TestNodeImpl, TestRunner};
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
        NodeImplementation,
    },
    types::VoteType,
};
use hotshot_types::{
    data::{LeafType, ProposalType, ValidatingLeaf, ValidatingProposal, ViewNumber},
    traits::{
        block_contents::dummy::{DummyBlock, DummyTransaction},
        election::{Membership, QuorumExchange},
        node_implementation::NodeType,
        state::ValidatingConsensus,
    },
    vote::QuorumVote,
};
use hotshot_types::message::Message;
use jf_primitives::{
    signatures::{
        bls::{BLSSignature, BLSVerKey},
        BLSSignatureScheme,
    },
    vrf::blsvrf::BLSVRFScheme,
};

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
}

struct StandardNodeImplType {}

type VrfMembership = VrfImpl<
    VrfTestTypes,
    ValidatingLeaf<VrfTestTypes>,
    BLSSignatureScheme<Param381>,
    BLSVRFScheme<Param381>,
    Hasher,
    Param381,
>;

type VrfCommunication = MemoryCommChannel<
    VrfTestTypes,
    StandardNodeImplType,
    ValidatingProposal<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
    QuorumVote<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
    VrfMembership,
>;

struct StaticNodeImplType {}

type StaticMembership =
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>;

type StaticCommunication = MemoryCommChannel<
    StaticCommitteeTestTypes,
    StaticNodeImplType,
    ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
>;

impl NodeImplementation<VrfTestTypes> for StandardNodeImplType {
    type Storage = MemoryStorage<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>;
    type Leaf = ValidatingLeaf<VrfTestTypes>;
    type QuorumExchange =
        QuorumExchange<VrfTestTypes, ValidatingLeaf<VrfTestTypes>, VrfMembership, VrfCommunication, Message<VrfTestTypes, Self>>;
}
/// type synonym for vrf committee election
/// with in-memory network
// pub type StandardNodeImplType = TestNodeImpl<
//     VrfTestTypes,
//     ValidatingLeaf<VrfTestTypes>,
//     ValidatingProposal<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
//     QuorumVote<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
//     MemoryCommChannel<
//         VrfTestTypes,
//         ValidatingProposal<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
//         QuorumVote<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
//         VrfImpl<
//             VrfTestTypes,
//             ValidatingLeaf<VrfTestTypes>,
//             BLSSignatureScheme<Param381>,
//             BLSVRFScheme<Param381>,
//             Hasher,
//             Param381,
//         >,
//     >,
//     MemoryStorage<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
//     VrfImpl<
//         VrfTestTypes,
//         ValidatingLeaf<VrfTestTypes>,
//         BLSSignatureScheme<Param381>,
//         BLSVRFScheme<Param381>,
//         Hasher,
//         Param381,
//     >,
// >;

/// type synonym for static committee

impl NodeImplementation<StaticCommitteeTestTypes> for StaticNodeImplType {
    type Storage =
        MemoryStorage<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>;
    type Leaf = ValidatingLeaf<StaticCommitteeTestTypes>;
    type QuorumExchange = QuorumExchange<
        StaticCommitteeTestTypes,
        ValidatingLeaf<StaticCommitteeTestTypes>,
        StaticMembership,
        StaticCommunication,
        Message<StaticCommitteeTestTypes, Self>
    >;
}
/// with in-memory network
// pub type StaticNodeImplType = TestNodeImpl<
//     StaticCommitteeTestTypes,
//     ValidatingLeaf<StaticCommitteeTestTypes>,
//     ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
//     QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
//     MemoryCommChannel<
//         StaticCommitteeTestTypes,
//         Self,
//         ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
//         QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
//         StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
//     >,
//     MemoryStorage<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
//     StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
// >;

/// type alias for the test runner type
pub type AppliedTestRunner<TYPES, LEAF, PROPOSAL, VOTE, MEMBERSHIP> =
    TestRunner<TYPES, AppliedTestNodeImpl<TYPES, LEAF, PROPOSAL, VOTE, MEMBERSHIP>>;
/// applied test runner (convenient type alias)
pub type AppliedTestNodeImpl<TYPES, LEAF, PROPOSAL, VOTE, MEMBERSHIP> = TestNodeImpl<
    TYPES,
    LEAF,
    PROPOSAL,
    VOTE,
    MemoryCommChannel<TYPES, Self, PROPOSAL, VOTE, MEMBERSHIP>,
    MemoryStorage<TYPES, LEAF>,
    MEMBERSHIP,
>;
