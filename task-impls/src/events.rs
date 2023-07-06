use hotshot_task::task::PassType;
use hotshot_types::certificate::{DACertificate, QuorumCertificate};
use hotshot_types::data::{DAProposal, ViewNumber};
use hotshot_types::message::Proposal;
use hotshot_types::message::ViewSyncMessageType;
use hotshot_types::traits::node_implementation::NodeImplementation;
use hotshot_types::traits::node_implementation::NodeType;
use hotshot_types::traits::node_implementation::QuorumProposalType;
use hotshot_types::vote::{DAVote, QuorumVote};

use crate::view_sync::ViewSyncPhase;

#[derive(Eq, Hash, PartialEq, Debug, Clone)]
pub enum SequencingHotShotEvent<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    Shutdown,
    QuorumProposalRecv(Proposal<QuorumProposalType<TYPES, I>>, TYPES::SignatureKey),
    QuorumVoteRecv(QuorumVote<TYPES, I::Leaf>, TYPES::SignatureKey),
    DAProposalRecv(Proposal<DAProposal<TYPES>>, TYPES::SignatureKey),
    DAVoteRecv(DAVote<TYPES, I::Leaf>, TYPES::SignatureKey),
    DACRecv(DACertificate<TYPES>),
    ViewSyncMessageRecv(ViewSyncMessageType<TYPES>),
    QuorumProposalSend(Proposal<QuorumProposalType<TYPES, I>>),
    QuorumVoteSend(QuorumVote<TYPES, I::Leaf>),
    DAProposalSend(Proposal<DAProposal<TYPES>>),
    DAVoteSend(DAVote<TYPES, I::Leaf>),
    QCSend(QuorumCertificate<TYPES, I::Leaf>),
    DACSend(DACertificate<TYPES>),
    ViewChange(ViewNumber),
    ViewSyncTimeout(ViewNumber, u64, ViewSyncPhase),
    ViewSyncMessageSend(ViewSyncMessageType<TYPES>),
    Timeout(ViewNumber),
}
impl<TYPES: NodeType, I: NodeImplementation<TYPES>> PassType for SequencingHotShotEvent<TYPES, I> {}
