use crate::task::PassType;
use hotshot_types::certificate::DACertificate;
use hotshot_types::certificate::QuorumCertificate;
use hotshot_types::data::DAProposal;
use hotshot_types::message::Proposal;
use hotshot_types::traits::node_implementation::NodeImplementation;
use hotshot_types::traits::node_implementation::NodeType;
use hotshot_types::traits::node_implementation::QuorumProposalType;
use hotshot_types::vote::{DAVote, QuorumVote};

#[derive(Eq, Hash, PartialEq, Debug, Clone)]
pub enum SequencingHotShotEvent<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    Shutdown,
    QuorumProposalRecv((Proposal<QuorumProposalType<TYPES, I>>, TYPES::SignatureKey)),
    QuorumVoteRecv(QuorumVote<TYPES, I::Leaf>, TYPES::SignatureKey),
    DAProposalRecv(Proposal<DAProposal<TYPES>>, TYPES::SignatureKey),
    DAVoteRecv(DAVote<TYPES, I::Leaf>, TYPES::SignatureKey),
    DACertificateRecv(DACertificate<TYPES>, TYPES::SignatureKey),
    ViewSyncMessage,
    QuorumProposalSend(Proposal<QuorumProposalType<TYPES, I>>),
    DAProposalSend(Proposal<DAProposal<TYPES>>),
    QuorumVoteSend(QuorumVote<TYPES, I::Leaf>),
    DAVoteSend(DAVote<TYPES, I::Leaf>),
    DACertificateSend(DACertificate<TYPES>),
    QCFormed(QuorumCertificate<TYPES, I::Leaf>),
    ViewChange,
    /// A view has timeouted out locally or we received/formed a timeout cert
    /// contains the view we are moving to
    Timeout(TYPES::Time),
}
impl<TYPES: NodeType, I: NodeImplementation<TYPES>> PassType for SequencingHotShotEvent<TYPES, I> {}
