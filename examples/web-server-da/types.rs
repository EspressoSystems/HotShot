use crate::infraDA::WebServerDARun;
use hotshot::traits::implementations::MemoryStorage;
use hotshot::{
    demos::sdemo::SDemoTypes,
    traits::{election::static_committee::GeneralStaticCommittee, implementations::WebCommChannel},
};
use hotshot_types::message::Message;
use hotshot_types::traits::{
    consensus_type::sequencing_consensus::SequencingConsensus,
    election::{QuorumExchange, CommitteeExchange},
    node_implementation::{NodeImplementation, SequencingExchanges},
};
use hotshot_types::{
    data::{SequencingLeaf, DAProposal, QuorumProposal},
    message::SequencingMessage,
    traits::node_implementation::NodeType,
    vote::{DAVote, QuorumVote},
};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeImpl {}

pub type ThisLeaf = SequencingLeaf<SDemoTypes>;
pub type ThisMembership =
    GeneralStaticCommittee<SDemoTypes, ThisLeaf, <SDemoTypes as NodeType>::SignatureKey>;
pub type DANetwork = WebCommChannel<
    SDemoTypes,
    NodeImpl,
    ThisDAProposal,
    ThisDAVote,
    ThisMembership,
>;
pub type QuorumNetwork = WebCommChannel<
    SDemoTypes,
    NodeImpl,
    ThisQuorumProposal,
    ThisQuorumVote,
    ThisMembership,
>;
pub type ThisDAProposal = DAProposal<SDemoTypes>;
pub type ThisDAVote = DAVote<SDemoTypes, ThisLeaf>;

pub type ThisQuorumProposal = QuorumProposal<SDemoTypes, ThisLeaf>;
pub type ThisQuorumVote = QuorumVote<SDemoTypes, ThisLeaf>;

impl NodeImplementation<SDemoTypes> for NodeImpl {
    type Storage = MemoryStorage<SDemoTypes, Self::Leaf>;
    type Leaf = SequencingLeaf<SDemoTypes>;
    type Exchanges = SequencingExchanges<
        SDemoTypes,
        Message<SDemoTypes, Self>,
        QuorumExchange<
            SDemoTypes,
            Self::Leaf,
            ThisQuorumProposal,
            ThisMembership,
            QuorumNetwork,
            Message<SDemoTypes, Self>,
        >,
        CommitteeExchange<
            SDemoTypes,
            ThisMembership,
            DANetwork,
            Message<SDemoTypes, Self>,
        >,
    >;
    type ConsensusMessage = SequencingMessage<SDemoTypes, Self>;
}
pub type ThisRun = WebServerDARun<SDemoTypes, NodeImpl, ThisMembership>;
