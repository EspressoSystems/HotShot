use hotshot_task::task::PassType;
use hotshot_types::certificate::{DACertificate, QuorumCertificate, ViewSyncCertificate};
use hotshot_types::data::{DAProposal, ViewNumber};
use hotshot_types::message::{Proposal, DataMessage};
use hotshot_types::traits::node_implementation::NodeImplementation;
use hotshot_types::traits::node_implementation::NodeType;
use hotshot_types::traits::node_implementation::QuorumProposalType;
use hotshot_types::traits::node_implementation::ViewSyncProposalType;
use hotshot_types::vote::ViewSyncVote;
use hotshot_types::vote::{DAVote, QuorumVote};

use crate::view_sync::ViewSyncPhase;

#[derive(Eq, Hash, PartialEq, Debug, Clone)]
pub enum SequencingHotShotEvent<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    Shutdown,
    QuorumProposalRecv(Proposal<QuorumProposalType<TYPES, I>>, TYPES::SignatureKey),
    QuorumVoteRecv(QuorumVote<TYPES, I::Leaf>),
    DAProposalRecv(Proposal<DAProposal<TYPES>>, TYPES::SignatureKey),
    DAVoteRecv(DAVote<TYPES>),
    DACRecv(DACertificate<TYPES>),
    QuorumProposalSend(Proposal<QuorumProposalType<TYPES, I>>, TYPES::SignatureKey),
    QuorumVoteSend(QuorumVote<TYPES, I::Leaf>),
    DAProposalSend(Proposal<DAProposal<TYPES>>, TYPES::SignatureKey),
    DAVoteSend(DAVote<TYPES>),
    QCFormed(QuorumCertificate<TYPES, I::Leaf>),
    DACSend(DACertificate<TYPES>),
    ViewChange(ViewNumber),
    ViewSyncTimeout(ViewNumber, u64, ViewSyncPhase),
    ViewSyncVoteSend(ViewSyncVote<TYPES>),
    ViewSyncCertificateSend(
        Proposal<ViewSyncProposalType<TYPES, I>>,
        TYPES::SignatureKey,
    ),
    ViewSyncVoteRecv(ViewSyncVote<TYPES>),
    ViewSyncCertificateRecv(Proposal<ViewSyncProposalType<TYPES, I>>),
    Timeout(ViewNumber),
    TransactionRecv(TYPES::Transaction),
    TransactionSend(TYPES::Transaction),

}
impl<TYPES: NodeType, I: NodeImplementation<TYPES>> PassType for SequencingHotShotEvent<TYPES, I> {}
