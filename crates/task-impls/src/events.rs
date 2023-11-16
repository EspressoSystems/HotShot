use crate::view_sync::ViewSyncPhase;

use commit::Commitment;
use either::Either;
use hotshot_types::{
    data::{DAProposal, Leaf, QuorumProposal, VidDisperse},
    message::Proposal,
    simple_certificate::{
        DACertificate2, QuorumCertificate2, TimeoutCertificate2, VIDCertificate2,
        ViewSyncCommitCertificate2, ViewSyncFinalizeCertificate2, ViewSyncPreCommitCertificate2,
    },
    simple_vote::{
        DAVote2, QuorumVote, TimeoutVote2, VIDVote2, ViewSyncCommitVote, ViewSyncFinalizeVote,
        ViewSyncPreCommitVote,
    },
    traits::{
        node_implementation::{
            CommitteeMembership, NodeImplementation, NodeType, QuorumMembership, VIDMembership,
            ViewSyncMembership,
        },
        BlockPayload,
    },
};

/// All of the possible events that can be passed between Sequecning `HotShot` tasks
#[derive(Eq, Hash, PartialEq, Debug, Clone)]
pub enum HotShotEvent<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Shutdown the task
    Shutdown,
    /// A quorum proposal has been received from the network; handled by the consensus task
    QuorumProposalRecv(Proposal<TYPES, QuorumProposal<TYPES>>, TYPES::SignatureKey),
    /// A quorum vote has been received from the network; handled by the consensus task
    QuorumVoteRecv(QuorumVote<TYPES, QuorumMembership<TYPES, I>>),
    /// A timeout vote recevied from the network; handled by consensus task
    TimeoutVoteRecv(TimeoutVote2<TYPES, QuorumMembership<TYPES, I>>),
    /// Send a timeout vote to the network; emitted by consensus task replicas
    TimeoutVoteSend(TimeoutVote2<TYPES, QuorumMembership<TYPES, I>>),
    /// A DA proposal has been received from the network; handled by the DA task
    DAProposalRecv(Proposal<TYPES, DAProposal<TYPES>>, TYPES::SignatureKey),
    /// A DA vote has been received by the network; handled by the DA task
    DAVoteRecv(DAVote2<TYPES, CommitteeMembership<TYPES, I>>),
    /// A Data Availability Certificate (DAC) has been recieved by the network; handled by the consensus task
    DACRecv(DACertificate2<TYPES>),
    /// Send a quorum proposal to the network; emitted by the leader in the consensus task
    QuorumProposalSend(Proposal<TYPES, QuorumProposal<TYPES>>, TYPES::SignatureKey),
    /// Send a quorum vote to the next leader; emitted by a replica in the consensus task after seeing a valid quorum proposal
    QuorumVoteSend(QuorumVote<TYPES, QuorumMembership<TYPES, I>>),
    /// Send a DA proposal to the DA committee; emitted by the DA leader (which is the same node as the leader of view v + 1) in the DA task
    DAProposalSend(Proposal<TYPES, DAProposal<TYPES>>, TYPES::SignatureKey),
    /// Send a DA vote to the DA leader; emitted by DA committee members in the DA task after seeing a valid DA proposal
    DAVoteSend(DAVote2<TYPES, CommitteeMembership<TYPES, I>>),
    /// The next leader has collected enough votes to form a QC; emitted by the next leader in the consensus task; an internal event only
    QCFormed(Either<QuorumCertificate2<TYPES>, TimeoutCertificate2<TYPES>>),
    /// The DA leader has collected enough votes to form a DAC; emitted by the DA leader in the DA task; sent to the entire network via the networking task
    DACSend(DACertificate2<TYPES>, TYPES::SignatureKey),
    /// The current view has changed; emitted by the replica in the consensus task or replica in the view sync task; received by almost all other tasks
    ViewChange(TYPES::Time),
    /// Timeout for the view sync protocol; emitted by a replica in the view sync task
    ViewSyncTimeout(TYPES::Time, u64, ViewSyncPhase),

    /// Receive a `ViewSyncPreCommitVote` from the network; received by a relay in the view sync task
    ViewSyncPreCommitVoteRecv(ViewSyncPreCommitVote<TYPES, ViewSyncMembership<TYPES, I>>),
    /// Receive a `ViewSyncCommitVote` from the network; received by a relay in the view sync task
    ViewSyncCommitVoteRecv(ViewSyncCommitVote<TYPES, ViewSyncMembership<TYPES, I>>),
    /// Receive a `ViewSyncFinalizeVote` from the network; received by a relay in the view sync task
    ViewSyncFinalizeVoteRecv(ViewSyncFinalizeVote<TYPES, ViewSyncMembership<TYPES, I>>),

    /// Send a `ViewSyncPreCommitVote` from the network; emitted by a replica in the view sync task
    ViewSyncPreCommitVoteSend(ViewSyncPreCommitVote<TYPES, ViewSyncMembership<TYPES, I>>),
    /// Send a `ViewSyncCommitVote` from the network; emitted by a replica in the view sync task
    ViewSyncCommitVoteSend(ViewSyncCommitVote<TYPES, ViewSyncMembership<TYPES, I>>),
    /// Send a `ViewSyncFinalizeVote` from the network; emitted by a replica in the view sync task
    ViewSyncFinalizeVoteSend(ViewSyncFinalizeVote<TYPES, ViewSyncMembership<TYPES, I>>),

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
        Commitment<TYPES::BlockPayload>,
        <TYPES::BlockPayload as BlockPayload>::Metadata,
    ),
    /// Event when the transactions task has a block formed
    BlockReady(
        TYPES::BlockPayload,
        <TYPES::BlockPayload as BlockPayload>::Metadata,
        TYPES::Time,
    ),
    /// Event when consensus decided on a leaf
    LeafDecided(Vec<Leaf<TYPES>>),
    /// Send VID shares to VID storage nodes; emitted by the DA leader
    ///
    /// Like [`DAProposalSend`].
    VidDisperseSend(Proposal<TYPES, VidDisperse<TYPES>>, TYPES::SignatureKey),
    /// Vid disperse data has been received from the network; handled by the DA task
    ///
    /// Like [`DAProposalRecv`].
    VidDisperseRecv(Proposal<TYPES, VidDisperse<TYPES>>, TYPES::SignatureKey),
    /// Send a VID vote to the VID leader; emitted by VID storage nodes in the DA task after seeing a valid VID dispersal
    ///
    /// Like [`DAVoteSend`]
    VidVoteSend(VIDVote2<TYPES, VIDMembership<TYPES, I>>),
    /// A VID vote has been received by the network; handled by the DA task
    ///
    /// Like [`DAVoteRecv`]
    VidVoteRecv(VIDVote2<TYPES, VIDMembership<TYPES, I>>),
    /// The VID leader has collected enough votes to form a VID cert; emitted by the VID leader in the DA task; sent to the entire network via the networking task
    ///
    /// Like [`DACSend`]
    VidCertSend(VIDCertificate2<TYPES>, TYPES::SignatureKey),
    /// A VID cert has been recieved by the network; handled by the consensus task
    ///
    /// Like [`DACRecv`]
    VidCertRecv(VIDCertificate2<TYPES>),
}
