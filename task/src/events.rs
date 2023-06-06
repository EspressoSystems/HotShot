use types::data::{DAProposal, ProposalType};
use types::vote::{DAVote, QuorumVote};

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
impl PassType for HotShotEvent {}