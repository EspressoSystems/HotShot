use crate::test_runner::TestRunner;
// use ark_bls12_381::Parameters as Param381;
// use blake3::Hasher;
use hotshot::{
    demos::vdemo::{VDemoBlock, VDemoState, VDemoTransaction},
    traits::{
        // dummy::DummyState,
        election::{
            static_committee::{StaticCommittee, StaticElectionConfig, StaticVoteToken},
            vrf::JfPubKey, // VRFStakeTableConfig, VRFVoteToken, VrfImpl},
        },
        implementations::{MemoryCommChannel, MemoryStorage},
        NodeImplementation,
    },
};
use hotshot_types::traits::election::ViewSyncExchange;
use hotshot_types::vote::ViewSyncVote;
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal, ViewNumber},
    message::ValidatingMessage,
    traits::{
        // block_contents::dummy::{DummyBlock, DummyTransaction},
        consensus_type::validating_consensus::ValidatingConsensus,
        election::QuorumExchange,
        node_implementation::{ChannelMaps, NodeType},
    },
    vote::QuorumVote,
};
use hotshot_types::{message::Message, traits::node_implementation::ValidatingExchanges};
use jf_primitives::signatures::BLSSignatureScheme;
use serde::{Deserialize, Serialize};

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
/// vrf test types
// pub struct VrfTestTypes;
// impl NodeType for VrfTestTypes {
//     // TODO (da) can this be SequencingConsensus?
//     type ConsensusType = ValidatingConsensus;
//     type Time = ViewNumber;
//     type BlockType = DummyBlock;
//     type SignatureKey = JfPubKey<BLSSignatureScheme>;
//     type VoteTokenType = VRFVoteToken<BLSVerKey<Param381>, BLSSignature<Param381>>;
//     type Transaction = DummyTransaction;
//     type ElectionConfigType = VRFStakeTableConfig;
//     type StateType = DummyState;
// }

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
    type SignatureKey = JfPubKey<BLSSignatureScheme>;
    type VoteTokenType = StaticVoteToken<JfPubKey<BLSSignatureScheme>>;
    type Transaction = VDemoTransaction;
    type ElectionConfigType = StaticElectionConfig;
    type StateType = VDemoState;
}

/// type alias for a "usable" node impl type
#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct StandardNodeImplType {}

// /// type alias for membership using vrf types
// pub type VrfMembership = VrfImpl<
//     VrfTestTypes,
//     ValidatingLeaf<VrfTestTypes>,
//     BLSSignatureScheme,
//     BLSVRFScheme<Param381>,
//     Hasher,
//     Param381,
// >;

// /// type alias for comm channel using vrf
// pub type VrfCommunication = MemoryCommChannel<
//     VrfTestTypes,
//     StandardNodeImplType,
//     ValidatingProposal<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
//     QuorumVote<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
//     VrfMembership,
// >;

/// type alias for static committee node
#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct StaticNodeImplType {}

type StaticMembership =
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>;

type StaticCommunication = MemoryCommChannel<
    StaticCommitteeTestTypes,
    StaticNodeImplType,
    ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
>;

type StaticCommunicationViewSync = MemoryCommChannel<
    StaticCommitteeTestTypes,
    StaticNodeImplType,
    ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    ViewSyncVote<StaticCommitteeTestTypes>,
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
>;

// impl NodeImplementation<VrfTestTypes> for StandardNodeImplType {
//     type Storage = MemoryStorage<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>;
//     type Leaf = ValidatingLeaf<VrfTestTypes>;
//     type Exchanges = ValidatingExchanges<
//         VrfTestTypes,
//         Message<VrfTestTypes, Self>,
//         QuorumExchange<
//             VrfTestTypes,
//             ValidatingLeaf<VrfTestTypes>,
//             ValidatingProposal<VrfTestTypes, ValidatingLeaf<VrfTestTypes>>,
//             VrfMembership,
//             VrfCommunication,
//             Message<VrfTestTypes, Self>,
//         >,
//     >;
//     type ConsensusMessage = ValidatingMessage<VrfTestTypes, Self>;
// }

impl NodeImplementation<StaticCommitteeTestTypes> for StaticNodeImplType {
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
            StaticMembership,
            StaticCommunication,
            Message<StaticCommitteeTestTypes, Self>,
        >,
        ViewSyncExchange<
            StaticCommitteeTestTypes,
            ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
            StaticMembership,
            StaticCommunicationViewSync,
            Message<StaticCommitteeTestTypes, Self>,
        >,
    >;
    type ConsensusMessage = ValidatingMessage<StaticCommitteeTestTypes, Self>;

    fn new_channel_maps(
        start_view: ViewNumber,
    ) -> (
        ChannelMaps<StaticCommitteeTestTypes, Self>,
        Option<ChannelMaps<StaticCommitteeTestTypes, Self>>,
    ) {
        (ChannelMaps::new(start_view), None)
    }
}

/// type alias for the test runner type
pub type AppliedTestRunner<TYPES, I> = TestRunner<TYPES, I>;
