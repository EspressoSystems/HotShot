use std::sync::Arc;

use either::Either;
use hotshot_types::{
    consensus::ProposalDependencyData,
    data::{DAProposal, Leaf, QuorumProposal, UpgradeProposal, VidDisperse, VidDisperseShare},
    message::Proposal,
    simple_certificate::{
        DACertificate, QuorumCertificate, TimeoutCertificate, UpgradeCertificate,
        ViewSyncCommitCertificate2, ViewSyncFinalizeCertificate2, ViewSyncPreCommitCertificate2,
    },
    simple_vote::{
        DAVote, QuorumVote, TimeoutVote, UpgradeVote, ViewSyncCommitVote, ViewSyncFinalizeVote,
        ViewSyncPreCommitVote,
    },
    traits::{block_contents::BuilderFee, node_implementation::NodeType, BlockPayload},
    utils::BuilderCommitment,
    vid::VidCommitment,
    vote::VoteDependencyData,
};
use vbs::version::Version;

use crate::view_sync::ViewSyncPhase;

/// Marker that the task completed
#[derive(Eq, Hash, PartialEq, Debug, Clone)]
pub struct HotShotTaskCompleted;

/// All of the possible events that can be passed between Sequecning `HotShot` tasks
#[derive(Eq, Hash, PartialEq, Debug, Clone)]
#[cfg_attr(feature = "rewind", derive(serde::Serialize, serde::Deserialize))]
#[serde(bound(deserialize = ""))]
#[allow(clippy::large_enum_variant)]
pub enum HotShotEvent<TYPES: NodeType> {
    /// Shutdown the task
    Shutdown,
    /// A quorum proposal has been received from the network; handled by the consensus task
    QuorumProposalRecv(Proposal<TYPES, QuorumProposal<TYPES>>, TYPES::SignatureKey),
    /// A quorum vote has been received from the network; handled by the consensus task
    QuorumVoteRecv(QuorumVote<TYPES>),
    /// A timeout vote recevied from the network; handled by consensus task
    TimeoutVoteRecv(TimeoutVote<TYPES>),
    /// Send a timeout vote to the network; emitted by consensus task replicas
    TimeoutVoteSend(TimeoutVote<TYPES>),
    /// A DA proposal has been received from the network; handled by the DA task
    DAProposalRecv(Proposal<TYPES, DAProposal<TYPES>>, TYPES::SignatureKey),
    /// A DA proposal has been validated; handled by the DA task and VID task
    DAProposalValidated(Proposal<TYPES, DAProposal<TYPES>>, TYPES::SignatureKey),
    /// A DA vote has been received by the network; handled by the DA task
    DAVoteRecv(DAVote<TYPES>),
    /// A Data Availability Certificate (DAC) has been recieved by the network; handled by the consensus task
    DACertificateRecv(DACertificate<TYPES>),
    /// A DAC is validated.
    DACertificateValidated(DACertificate<TYPES>),
    /// Send a quorum proposal to the network; emitted by the leader in the consensus task
    QuorumProposalSend(Proposal<TYPES, QuorumProposal<TYPES>>, TYPES::SignatureKey),
    /// Send a quorum vote to the next leader; emitted by a replica in the consensus task after seeing a valid quorum proposal
    QuorumVoteSend(QuorumVote<TYPES>),
    /// All dependencies for the quorum vote are validated.
    QuorumVoteDependenciesValidated(TYPES::Time),
    /// A quorum proposal with the given parent leaf is validated.
    QuorumProposalValidated(QuorumProposal<TYPES>, Leaf<TYPES>),
    /// Send a DA proposal to the DA committee; emitted by the DA leader (which is the same node as the leader of view v + 1) in the DA task
    DAProposalSend(Proposal<TYPES, DAProposal<TYPES>>, TYPES::SignatureKey),
    /// Send a DA vote to the DA leader; emitted by DA committee members in the DA task after seeing a valid DA proposal
    DAVoteSend(DAVote<TYPES>),
    /// The next leader has collected enough votes to form a QC; emitted by the next leader in the consensus task; an internal event only
    QCFormed(Either<QuorumCertificate<TYPES>, TimeoutCertificate<TYPES>>),
    /// The DA leader has collected enough votes to form a DAC; emitted by the DA leader in the DA task; sent to the entire network via the networking task
    DACSend(DACertificate<TYPES>, TYPES::SignatureKey),
    /// The current view has changed; emitted by the replica in the consensus task or replica in the view sync task; received by almost all other tasks
    ViewChange(TYPES::Time),
    /// Timeout for the view sync protocol; emitted by a replica in the view sync task
    ViewSyncTimeout(TYPES::Time, u64, ViewSyncPhase),

    /// Receive a `ViewSyncPreCommitVote` from the network; received by a relay in the view sync task
    ViewSyncPreCommitVoteRecv(ViewSyncPreCommitVote<TYPES>),
    /// Receive a `ViewSyncCommitVote` from the network; received by a relay in the view sync task
    ViewSyncCommitVoteRecv(ViewSyncCommitVote<TYPES>),
    /// Receive a `ViewSyncFinalizeVote` from the network; received by a relay in the view sync task
    ViewSyncFinalizeVoteRecv(ViewSyncFinalizeVote<TYPES>),

    /// Send a `ViewSyncPreCommitVote` from the network; emitted by a replica in the view sync task
    ViewSyncPreCommitVoteSend(ViewSyncPreCommitVote<TYPES>),
    /// Send a `ViewSyncCommitVote` from the network; emitted by a replica in the view sync task
    ViewSyncCommitVoteSend(ViewSyncCommitVote<TYPES>),
    /// Send a `ViewSyncFinalizeVote` from the network; emitted by a replica in the view sync task
    ViewSyncFinalizeVoteSend(ViewSyncFinalizeVote<TYPES>),

    /// Receive a `ViewSyncPreCommitCertificate2` from the network; received by a replica in the view sync task
    ViewSyncPreCommitCertificate2Recv(ViewSyncPreCommitCertificate2<TYPES>),
    /// Receive a `ViewSyncCommitCertificate2` from the network; received by a replica in the view sync task
    ViewSyncCommitCertificate2Recv(ViewSyncCommitCertificate2<TYPES>),
    /// Receive a `ViewSyncFinalizeCertificate2` from the network; received by a replica in the view sync task
    ViewSyncFinalizeCertificate2Recv(ViewSyncFinalizeCertificate2<TYPES>),

    /// Send a `ViewSyncPreCommitCertificate2` from the network; emitted by a relay in the view sync task
    ViewSyncPreCommitCertificate2Send(ViewSyncPreCommitCertificate2<TYPES>, TYPES::SignatureKey),
    /// Send a `ViewSyncCommitCertificate2` from the network; emitted by a relay in the view sync task
    ViewSyncCommitCertificate2Send(ViewSyncCommitCertificate2<TYPES>, TYPES::SignatureKey),
    /// Send a `ViewSyncFinalizeCertificate2` from the network; emitted by a relay in the view sync task
    ViewSyncFinalizeCertificate2Send(ViewSyncFinalizeCertificate2<TYPES>, TYPES::SignatureKey),

    /// Trigger the start of the view sync protocol; emitted by view sync task; internal trigger only
    ViewSyncTrigger(TYPES::Time),
    /// A consensus view has timed out; emitted by a replica in the consensus task; received by the view sync task; internal event only
    Timeout(TYPES::Time),
    /// Receive transactions from the network
    TransactionsRecv(Vec<TYPES::Transaction>),
    /// Send transactions to the network
    TransactionSend(TYPES::Transaction, TYPES::SignatureKey),
    /// Event to send block payload commitment and metadata from DA leader to the quorum; internal event only
    SendPayloadCommitmentAndMetadata(
        VidCommitment,
        BuilderCommitment,
        <TYPES::BlockPayload as BlockPayload>::Metadata,
        TYPES::Time,
        BuilderFee<TYPES>,
    ),
    /// Event when the transactions task has sequenced transactions. Contains the encoded transactions, the metadata, and the view number
    BlockRecv(
        Arc<[u8]>,
        <TYPES::BlockPayload as BlockPayload>::Metadata,
        TYPES::Time,
        BuilderFee<TYPES>,
    ),
    /// Event when the transactions task has a block formed
    BlockReady(VidDisperse<TYPES>, TYPES::Time),
    /// Event when consensus decided on a leaf
    LeafDecided(Vec<Leaf<TYPES>>),
    /// Send VID shares to VID storage nodes; emitted by the DA leader
    ///
    /// Like [`HotShotEvent::DAProposalSend`].
    VidDisperseSend(Proposal<TYPES, VidDisperse<TYPES>>, TYPES::SignatureKey),
    /// Vid disperse share has been received from the network; handled by the consensus task
    ///
    /// Like [`HotShotEvent::DAProposalRecv`].
    VIDShareRecv(Proposal<TYPES, VidDisperseShare<TYPES>>),
    /// VID share data is validated.
    VIDShareValidated(Proposal<TYPES, VidDisperseShare<TYPES>>),
    /// Upgrade proposal has been received from the network
    UpgradeProposalRecv(Proposal<TYPES, UpgradeProposal<TYPES>>, TYPES::SignatureKey),
    /// Upgrade proposal has been sent to the network
    UpgradeProposalSend(Proposal<TYPES, UpgradeProposal<TYPES>>, TYPES::SignatureKey),
    /// Upgrade vote has been received from the network
    UpgradeVoteRecv(UpgradeVote<TYPES>),
    /// Upgrade vote has been sent to the network
    UpgradeVoteSend(UpgradeVote<TYPES>),
    /// Upgrade certificate has been sent to the network
    UpgradeCertificateFormed(UpgradeCertificate<TYPES>),
    /// HotShot was upgraded, with a new network version.
    VersionUpgrade(Version),
    /// Initiate a proposal right now for a provided view.
    ProposeNow(TYPES::Time, ProposalDependencyData<TYPES>),
    /// Initiate a vote right now for the designated view.
    VoteNow(TYPES::Time, VoteDependencyData<TYPES>),
}

impl<TYPES: NodeType> std::fmt::Display for HotShotEvent<TYPES> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HotShotEvent::Shutdown => write!(f, "Shutdown"),
            HotShotEvent::QuorumProposalRecv(_, _) => write!(f, "QuorumProposalRecv"),
            HotShotEvent::QuorumVoteRecv(_) => write!(f, "QuorumVoteRecv"),
            HotShotEvent::TimeoutVoteRecv(_) => write!(f, "TimeoutVoteRecv"),
            HotShotEvent::TimeoutVoteSend(_) => write!(f, "TimeoutVoteSend"),
            HotShotEvent::DAProposalRecv(_, _) => write!(f, "DAProposalRecv"),
            HotShotEvent::DAProposalValidated(_, _) => write!(f, "DAProposalValidated"),
            HotShotEvent::DAVoteRecv(_) => write!(f, "DAVoteRecv"),
            HotShotEvent::DACertificateRecv(_) => write!(f, "DACertificateRecv"),
            HotShotEvent::DACertificateValidated(_) => write!(f, "DACertificateValidated"),
            HotShotEvent::QuorumProposalSend(_, _) => write!(f, "QuorumProposalSend"),
            HotShotEvent::QuorumVoteSend(_) => write!(f, "QuorumVoteSend"),
            HotShotEvent::QuorumVoteDependenciesValidated(_) => {
                write!(f, "QuorumVoteDependenciesValidated")
            }
            HotShotEvent::QuorumProposalValidated(_, _) => write!(f, "QuorumProposalValidated"),
            HotShotEvent::DAProposalSend(_, _) => write!(f, "DAProposalSend"),
            HotShotEvent::DAVoteSend(_) => write!(f, "DAVoteSend"),
            HotShotEvent::QCFormed(_) => write!(f, "QCFormed"),
            HotShotEvent::DACSend(_, _) => write!(f, "DACSend"),
            HotShotEvent::ViewChange(_) => write!(f, "ViewChange"),
            HotShotEvent::ViewSyncTimeout(_, _, _) => write!(f, "ViewSyncTimeout"),
            HotShotEvent::ViewSyncPreCommitVoteRecv(_) => write!(f, "ViewSyncPreCommitVoteRecv"),
            HotShotEvent::ViewSyncCommitVoteRecv(_) => write!(f, "ViewSyncCommitVoteRecv"),
            HotShotEvent::ViewSyncFinalizeVoteRecv(_) => write!(f, "ViewSyncFinalizeVoteRecv"),
            HotShotEvent::ViewSyncPreCommitVoteSend(_) => write!(f, "ViewSyncPreCommitVoteSend"),
            HotShotEvent::ViewSyncCommitVoteSend(_) => write!(f, "ViewSyncCommitVoteSend"),
            HotShotEvent::ViewSyncFinalizeVoteSend(_) => write!(f, "ViewSyncFinalizeVoteSend"),
            HotShotEvent::ViewSyncPreCommitCertificate2Recv(_) => {
                write!(f, "ViewSyncPreCommitCertificate2Recv")
            }
            HotShotEvent::ViewSyncCommitCertificate2Recv(_) => {
                write!(f, "ViewSyncCommitCertificate2Recv")
            }
            HotShotEvent::ViewSyncFinalizeCertificate2Recv(_) => {
                write!(f, "ViewSyncFinalizeCertificate2Recv")
            }
            HotShotEvent::ViewSyncPreCommitCertificate2Send(_, _) => {
                write!(f, "ViewSyncPreCommitCertificate2Send")
            }
            HotShotEvent::ViewSyncCommitCertificate2Send(_, _) => {
                write!(f, "ViewSyncCommitCertificate2Send")
            }
            HotShotEvent::ViewSyncFinalizeCertificate2Send(_, _) => {
                write!(f, "ViewSyncFinalizeCertificate2Send")
            }
            HotShotEvent::ViewSyncTrigger(_) => write!(f, "ViewSyncTrigger"),
            HotShotEvent::Timeout(_) => write!(f, "Timeout"),
            HotShotEvent::TransactionsRecv(_) => write!(f, "TransactionsRecv"),
            HotShotEvent::TransactionSend(_, _) => write!(f, "TransactionSend"),
            HotShotEvent::SendPayloadCommitmentAndMetadata(_, _, _, _, _) => {
                write!(f, "SendPayloadCommitmentAndMetadata")
            }
            HotShotEvent::BlockRecv(_, _, _, _) => write!(f, "BlockRecv"),
            HotShotEvent::BlockReady(_, _) => write!(f, "BlockReady"),
            HotShotEvent::LeafDecided(_) => write!(f, "LeafDecided"),
            HotShotEvent::VidDisperseSend(_, _) => write!(f, "VidDisperseSend"),
            HotShotEvent::VIDShareRecv(_) => write!(f, "VIDShareRecv"),
            HotShotEvent::VIDShareValidated(_) => write!(f, "VIDShareValidated"),
            HotShotEvent::UpgradeProposalRecv(_, _) => write!(f, "UpgradeProposalRecv"),
            HotShotEvent::UpgradeProposalSend(_, _) => write!(f, "UpgradeProposalSend"),
            HotShotEvent::UpgradeVoteRecv(_) => write!(f, "UpgradeVoteRecv"),
            HotShotEvent::UpgradeVoteSend(_) => write!(f, "UpgradeVoteSend"),
            HotShotEvent::UpgradeCertificateFormed(_) => write!(f, "UpgradeCertificateFormed"),
            HotShotEvent::VersionUpgrade(_) => write!(f, "VersionUpgrade"),
            HotShotEvent::ProposeNow(_, _) => write!(f, "ProposeNow"),
            HotShotEvent::VoteNow(_, _) => write!(f, "VoteNow"),
        }
    }
}
