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
use hotshot_types::message::{Message, SequencingMessage};
use hotshot_types::traits::election::ViewSyncExchange;
use hotshot_types::vote::QuorumVote;
use hotshot_types::vote::ViewSyncVote;
use hotshot_types::{certificate::ViewSyncCertificate, data::QuorumProposal};
use hotshot_types::{
    data::{DAProposal, SequencingLeaf, ViewNumber},
    traits::{
        consensus_type::sequencing_consensus::SequencingConsensus,
        election::{CommitteeExchange, QuorumExchange},
        node_implementation::{ChannelMaps, NodeType, SequencingExchanges},
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

#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct SequencingMemoryImpl;

#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct SequencingLibp2pImpl;

type StaticMembership =
StaticCommittee<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;

type StaticMemoryDAComm = MemoryCommChannel<
    SequencingTestTypes,
    SequencingMemoryImpl,
    DAProposal<SequencingTestTypes>,
    DAVote<SequencingTestTypes>,
    StaticMembership,
>;

type StaticLibp2pDAComm = Libp2pCommChannel<
    SequencingTestTypes,
    SequencingLibp2pImpl,
    DAProposal<SequencingTestTypes>,
    DAVote<SequencingTestTypes>,
    StaticMembership,
>;

type StaticMemoryQuorumComm = MemoryCommChannel<
    SequencingTestTypes,
    SequencingMemoryImpl,
    QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    StaticMembership,
>;

type StaticLibp2pQuorumComm = Libp2pCommChannel<
    SequencingTestTypes,
    SequencingLibp2pImpl,
    QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
    StaticMembership,
>;

type StaticMemoryViewSyncComm = MemoryCommChannel<
    SequencingTestTypes,
    SequencingMemoryImpl,
    ViewSyncCertificate<SequencingTestTypes>,
    ViewSyncVote<SequencingTestTypes>,
    StaticMembership,
>;

type StaticLibp2pViewSyncComm = Libp2pCommChannel<
    SequencingTestTypes,
    SequencingLibp2pImpl,
    ViewSyncCertificate<SequencingTestTypes>,
    ViewSyncVote<SequencingTestTypes>,
    StaticMembership,
>;

impl NodeImplementation<SequencingTestTypes> for SequencingLibp2pImpl {
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
            StaticLibp2pQuorumComm,
            Message<SequencingTestTypes, Self>,
            >,
            CommitteeExchange<
                SequencingTestTypes,
                StaticMembership,
                StaticLibp2pDAComm,
                Message<SequencingTestTypes, Self>,
                >,
                ViewSyncExchange<
                    SequencingTestTypes,
                    ViewSyncCertificate<SequencingTestTypes>,
                    StaticMembership,
                    StaticLibp2pViewSyncComm,
                    Message<SequencingTestTypes, Self>,
                    >,
                >;
    type ConsensusMessage = SequencingMessage<SequencingTestTypes, Self>;

    fn new_channel_maps(
        start_view: ViewNumber,
        ) -> (
            ChannelMaps<SequencingTestTypes, Self>,
            Option<ChannelMaps<SequencingTestTypes, Self>>,
            ) {
            (
                ChannelMaps::new(start_view),
                Some(ChannelMaps::new(start_view)),
                )
        }
}


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
            StaticMemoryQuorumComm,
            Message<SequencingTestTypes, Self>,
            >,
            CommitteeExchange<
                SequencingTestTypes,
                StaticMembership,
                StaticMemoryDAComm,
                Message<SequencingTestTypes, Self>,
                >,
                ViewSyncExchange<
                    SequencingTestTypes,
                    ViewSyncCertificate<SequencingTestTypes>,
                    StaticMembership,
                    StaticMemoryViewSyncComm,
                    Message<SequencingTestTypes, Self>,
                    >,
                    >;
    type ConsensusMessage = SequencingMessage<SequencingTestTypes, Self>;

    fn new_channel_maps(
        start_view: ViewNumber,
        ) -> (
            ChannelMaps<SequencingTestTypes, Self>,
            Option<ChannelMaps<SequencingTestTypes, Self>>,
            ) {
            (
                ChannelMaps::new(start_view),
                Some(ChannelMaps::new(start_view)),
                )
        }
}
