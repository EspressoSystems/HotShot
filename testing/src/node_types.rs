use std::sync::Arc;

use hotshot::{
    demos::sdemo::{SDemoBlock, SDemoState, SDemoTransaction},
    traits::{
        election::{
            static_committee::{StaticCommittee, StaticElectionConfig, StaticVoteToken},
            vrf::JfPubKey,
        },
        implementations::{
            Libp2pCommChannel, MemoryCommChannel, MemoryNetwork, MemoryStorage, WebCommChannel,
            WebServerWithFallbackCommChannel,
        },
        NodeImplementation,
    },
};
use hotshot_types::traits::election::ViewSyncExchange;
use hotshot_types::vote::QuorumVote;
use hotshot_types::vote::ViewSyncVote;
use hotshot_types::{certificate::ViewSyncCertificate, data::QuorumProposal};
use hotshot_types::{
    data::{DAProposal, SequencingLeaf, ViewNumber},
    traits::{
        election::{CommitteeExchange, QuorumExchange},
        node_implementation::{ChannelMaps, NodeType, SequencingExchanges},
    },
    vote::DAVote,
};
use hotshot_types::{
    message::{Message, SequencingMessage},
    traits::{
        network::{TestableChannelImplementation, TestableNetworkingImplementation},
        node_implementation::TestableExchange,
    },
};
use jf_primitives::signatures::BLSSignatureScheme;
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
pub struct SequencingTestTypes;
impl NodeType for SequencingTestTypes {
    type Time = ViewNumber;
    type BlockType = SDemoBlock;
    type SignatureKey = JfPubKey<BLSSignatureScheme>;
    type VoteTokenType = StaticVoteToken<Self::SignatureKey>;
    type Transaction = SDemoTransaction;
    type ElectionConfigType = StaticElectionConfig;
    type StateType = SDemoState;
}

#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct SequencingMemoryImpl;

#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct SequencingLibp2pImpl;

#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct SequencingWebImpl;

#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct StaticFallbackImpl;

type StaticMembership = StaticCommittee<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;

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

type StaticWebDAComm = WebCommChannel<
    SequencingTestTypes,
    SequencingWebImpl,
    DAProposal<SequencingTestTypes>,
    DAVote<SequencingTestTypes>,
    StaticMembership,
>;

type StaticFallbackComm =
    WebServerWithFallbackCommChannel<SequencingTestTypes, StaticFallbackImpl, StaticMembership>;

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

type StaticWebQuorumComm = WebCommChannel<
    SequencingTestTypes,
    SequencingWebImpl,
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

type StaticWebViewSyncComm = WebCommChannel<
    SequencingTestTypes,
    SequencingWebImpl,
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

pub type SequencingMemoryExchange = SequencingExchanges<
    SequencingTestTypes,
    Message<SequencingTestTypes, SequencingMemoryImpl>,
    QuorumExchange<
        SequencingTestTypes,
        <SequencingMemoryImpl as NodeImplementation<SequencingTestTypes>>::Leaf,
        QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
        StaticMembership,
        StaticMemoryQuorumComm,
        Message<SequencingTestTypes, SequencingMemoryImpl>,
    >,
    CommitteeExchange<
        SequencingTestTypes,
        StaticMembership,
        StaticMemoryDAComm,
        Message<SequencingTestTypes, SequencingMemoryImpl>,
    >,
    ViewSyncExchange<
        SequencingTestTypes,
        ViewSyncCertificate<SequencingTestTypes>,
        StaticMembership,
        StaticMemoryViewSyncComm,
        Message<SequencingTestTypes, SequencingMemoryImpl>,
    >,
>;

impl
    TestableExchange<
        SequencingTestTypes,
        <SequencingMemoryImpl as NodeImplementation<SequencingTestTypes>>::Leaf,
        Message<SequencingTestTypes, SequencingMemoryImpl>,
    > for SequencingMemoryExchange
{
    fn gen_comm_channels(
        expected_node_count: usize,
        num_bootstrap: usize,
        da_committee_size: usize,
    ) -> Box<
        dyn Fn(
                u64,
            ) -> (
                <Self::QuorumExchange as hotshot_types::traits::election::ConsensusExchange<
                    SequencingTestTypes,
                    Message<SequencingTestTypes, SequencingMemoryImpl>,
                >>::Networking,
                <Self::CommitteeExchange as hotshot_types::traits::election::ConsensusExchange<
                    SequencingTestTypes,
                    Message<SequencingTestTypes, SequencingMemoryImpl>,
                >>::Networking,
                <Self::ViewSyncExchange as hotshot_types::traits::election::ConsensusExchange<
                    SequencingTestTypes,
                    Message<SequencingTestTypes, SequencingMemoryImpl>,
                >>::Networking,
            ) + 'static,
    > {
        let network_generator = Arc::new(<MemoryNetwork<
            Message<SequencingTestTypes, SequencingMemoryImpl>,
            <SequencingTestTypes as NodeType>::SignatureKey,
        > as TestableNetworkingImplementation<
            SequencingTestTypes,
            Message<SequencingTestTypes, SequencingMemoryImpl>,
        >>::generator(
            expected_node_count,
            num_bootstrap,
            0,
            da_committee_size,
            false,
        ));
        let network_da_generator = Arc::new(<MemoryNetwork<
            Message<SequencingTestTypes, SequencingMemoryImpl>,
            <SequencingTestTypes as NodeType>::SignatureKey,
        > as TestableNetworkingImplementation<
            SequencingTestTypes,
            Message<SequencingTestTypes, SequencingMemoryImpl>,
        >>::generator(
            expected_node_count,
            num_bootstrap,
            1,
            da_committee_size,
            true,
        ));
        Box::new(move |id| {
            let network = Arc::new(network_generator(id));
            let network_da = Arc::new(network_da_generator(id));
            let quorum_chan = <<Self::QuorumExchange as hotshot_types::traits::election::ConsensusExchange<SequencingTestTypes, Message<SequencingTestTypes, SequencingMemoryImpl>>>::Networking as TestableChannelImplementation<_, _, _, _, _, _>>::generate_network()(network.clone());
            let committee_chan = <<Self::CommitteeExchange as hotshot_types::traits::election::ConsensusExchange<SequencingTestTypes, Message<SequencingTestTypes, SequencingMemoryImpl>>>::Networking as TestableChannelImplementation<_, _, _, _, _, _>>::generate_network()(network_da);
            let view_sync_chan = <<Self::ViewSyncExchange as hotshot_types::traits::election::ConsensusExchange<SequencingTestTypes, Message<SequencingTestTypes, SequencingMemoryImpl>>>::Networking as TestableChannelImplementation<_, _, _, _, _, _>>::generate_network()(network);

            (quorum_chan, committee_chan, view_sync_chan)
        })
    }
}

impl NodeImplementation<SequencingTestTypes> for SequencingMemoryImpl {
    type Storage = MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;
    type Leaf = SequencingLeaf<SequencingTestTypes>;
    type Exchanges = SequencingMemoryExchange;
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

// man these generics are big oof
// they're a LOT
// when are we getting HKT for rust
// smh my head

impl NodeImplementation<SequencingTestTypes> for SequencingWebImpl {
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
            StaticWebQuorumComm,
            Message<SequencingTestTypes, Self>,
        >,
        CommitteeExchange<
            SequencingTestTypes,
            StaticMembership,
            StaticWebDAComm,
            Message<SequencingTestTypes, Self>,
        >,
        ViewSyncExchange<
            SequencingTestTypes,
            ViewSyncCertificate<SequencingTestTypes>,
            StaticMembership,
            StaticWebViewSyncComm,
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

impl NodeImplementation<SequencingTestTypes> for StaticFallbackImpl {
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
            StaticFallbackComm,
            Message<SequencingTestTypes, Self>,
        >,
        CommitteeExchange<
            SequencingTestTypes,
            StaticMembership,
            StaticFallbackComm,
            Message<SequencingTestTypes, Self>,
        >,
        ViewSyncExchange<
            SequencingTestTypes,
            ViewSyncCertificate<SequencingTestTypes>,
            StaticMembership,
            StaticFallbackComm,
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
