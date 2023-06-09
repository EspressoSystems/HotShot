use crate::task::PassType;
use hotshot_types::data::{DAProposal, ProposalType};
use hotshot_types::message::Proposal;
use hotshot_types::traits::node_implementation::NodeImplementation;
use hotshot_types::traits::node_implementation::NodeType;
use hotshot_types::traits::node_implementation::QuorumProposalType;
use hotshot_types::vote::{DAVote, QuorumVote};

#[derive(Debug, Clone)]
pub enum SequencingHotShotEvent<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    Shutdown,
    QuorumProposalRecv((Proposal<QuorumProposalType<TYPES, I>>, TYPES::SignatureKey)),
    QuorumVoteRecv(QuorumVote<TYPES, I::Leaf>, TYPES::SignatureKey),
    DAProposalRecv(Proposal<DAProposal<TYPES>>, TYPES::SignatureKey),
    DAVoteRecv(DAVote<TYPES, I::Leaf>, TYPES::SignatureKey),
    ViewSyncMessage,
    QuorumProposalSend(Proposal<QuorumProposalType<TYPES, I>>),
    DAProposalSend(Proposal<DAProposal<TYPES>>),
    VoteSend(QuorumVote<TYPES, I::Leaf>),
    DAVoteSend(DAVote<TYPES, I::Leaf>),
    ViewChange,
    Timeout,
}
impl<TYPES: NodeType, I: NodeImplementation<TYPES>> PassType for SequencingHotShotEvent<TYPES, I> {}
