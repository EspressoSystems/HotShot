use crate::infra::WebServerRun;
use hotshot::traits::implementations::MemoryStorage;
use hotshot::{
    demos::sdemo::SDemoTypes,
    traits::{election::static_committee::GeneralStaticCommittee, implementations::WebCommChannel},
};
use hotshot_types::message::Message;
use hotshot_types::traits::{
    consensus_type::sequencing_consensus::SequencingConsensus,
    election::QuorumExchange,
    node_implementation::{NodeImplementation, SequencingExchanges},
};
use hotshot_types::{
    data::{SequencingLeaf, DAProposal},
    message::SequencingMessage,
    traits::node_implementation::NodeType,
    vote::DAVote,
};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeImpl {}

pub type ThisLeaf = SequencingLeaf<SDemoTypes>;
pub type ThisMembership =
    GeneralStaticCommittee<SDemoTypes, ThisLeaf, <SDemoTypes as NodeType>::SignatureKey>;
pub type ThisNetwork = WebCommChannel<
    SequencingConsensus,
    SDemoTypes,
    NodeImpl,
    ThisProposal,
    ThisVote,
    ThisMembership,
>;
pub type ThisProposal = SequencingProposal<SDemoTypes, ThisLeaf>;
pub type ThisVote = DAVote<SDemoTypes, ThisLeaf>;

impl NodeImplementation<SDemoTypes> for NodeImpl {
    type Storage = MemoryStorage<SDemoTypes, Self::Leaf>;
    type Leaf = SequencingLeaf<SDemoTypes>;
    type Exchanges = SequencingExchanges<
        SDemoTypes,
        Message<SDemoTypes, Self>,
        CommitteeExchange<
            SDemoTypes,
            Self::Leaf,
            ThisProposal,
            ThisMembership,
            ThisNetwork,
            Message<SDemoTypes, Self>,
        >,
    >;
    type ConsensusMessage = SequencingMessage<SDemoTypes, Self>;
}
pub type ThisRun = WebServerRun<SDemoTypes, NodeImpl, ThisMembership>;
