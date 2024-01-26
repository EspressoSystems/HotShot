use crate::events::{HotShotEvent, HotShotTaskCompleted};
use async_broadcast::Sender;
use either::Either::{self, Left, Right};
use hotshot_constants::PROGRAM_PROTOCOL_VERSION;

use hotshot_types::{
    message::{
        CommitteeConsensusMessage, GeneralConsensusMessage, Message, MessageKind, SequencingMessage,
    },
    traits::{
        election::Membership,
        network::{CommunicationChannel, TransmitType},
        node_implementation::NodeType,
    },
    vote::{HasViewNumber, Vote},
};
use task::task::{Task, TaskState};
use tracing::error;
use tracing::instrument;

/// the type of network task
#[derive(Clone, Copy, Debug)]
pub enum NetworkTaskKind {
    /// quorum: the normal "everyone" committee
    Quorum,
    /// da committee
    Committee,
    /// view sync
    ViewSync,
    /// vid
    VID,
}

/// the network message task state
#[derive(Clone)]
pub struct NetworkMessageTaskState<TYPES: NodeType> {
    pub event_stream: Sender<HotShotEvent<TYPES>>,
}

impl<TYPES: NodeType> TaskState for NetworkMessageTaskState<TYPES> {
    type Event = Vec<Message<TYPES>>;
    type Result = ();

    async fn handle_event(event: Self::Event, task: &mut Task<Self>) -> Option<()>
    where
        Self: Sized,
    {
        task.state_mut().handle_messages(event).await;
        None
    }

    fn should_shutdown(_event: &Self::Event) -> bool {
        false
    }
}

impl<TYPES: NodeType> NetworkMessageTaskState<TYPES> {
    /// Handle the message.
    pub async fn handle_messages(&mut self, messages: Vec<Message<TYPES>>) {
        // We will send only one event for a vector of transactions.
        let mut transactions = Vec::new();
        for message in messages {
            let sender = message.sender;
            match message.kind {
                MessageKind::Consensus(consensus_message) => {
                    let event = match consensus_message.0 {
                        Either::Left(general_message) => match general_message {
                            GeneralConsensusMessage::Proposal(proposal) => {
                                HotShotEvent::QuorumProposalRecv(proposal, sender)
                            }
                            GeneralConsensusMessage::Vote(vote) => {
                                HotShotEvent::QuorumVoteRecv(vote.clone())
                            }
                            GeneralConsensusMessage::ViewSyncPreCommitVote(view_sync_message) => {
                                HotShotEvent::ViewSyncPreCommitVoteRecv(view_sync_message)
                            }
                            GeneralConsensusMessage::ViewSyncPreCommitCertificate(
                                view_sync_message,
                            ) => HotShotEvent::ViewSyncPreCommitCertificate2Recv(view_sync_message),

                            GeneralConsensusMessage::ViewSyncCommitVote(view_sync_message) => {
                                HotShotEvent::ViewSyncCommitVoteRecv(view_sync_message)
                            }
                            GeneralConsensusMessage::ViewSyncCommitCertificate(
                                view_sync_message,
                            ) => HotShotEvent::ViewSyncCommitCertificate2Recv(view_sync_message),

                            GeneralConsensusMessage::ViewSyncFinalizeVote(view_sync_message) => {
                                HotShotEvent::ViewSyncFinalizeVoteRecv(view_sync_message)
                            }
                            GeneralConsensusMessage::ViewSyncFinalizeCertificate(
                                view_sync_message,
                            ) => HotShotEvent::ViewSyncFinalizeCertificate2Recv(view_sync_message),

                            GeneralConsensusMessage::TimeoutVote(message) => {
                                HotShotEvent::TimeoutVoteRecv(message)
                            }
                            GeneralConsensusMessage::UpgradeCertificate(message) => {
                                HotShotEvent::UpgradeCertificateRecv(message)
                            }
                            GeneralConsensusMessage::UpgradeProposal(message) => {
                                HotShotEvent::UpgradeProposalRecv(message)
                            }
                            GeneralConsensusMessage::UpgradeVote(message) => {
                                HotShotEvent::UpgradeVoteRecv(message)
                            }
                            GeneralConsensusMessage::InternalTrigger(_) => {
                                error!("Got unexpected message type in network task!");
                                return;
                            }
                        },
                        Either::Right(committee_message) => match committee_message {
                            CommitteeConsensusMessage::DAProposal(proposal) => {
                                HotShotEvent::DAProposalRecv(proposal, sender)
                            }
                            CommitteeConsensusMessage::DAVote(vote) => {
                                HotShotEvent::DAVoteRecv(vote.clone())
                            }
                            CommitteeConsensusMessage::DACertificate(cert) => {
                                HotShotEvent::DACRecv(cert)
                            }
                            CommitteeConsensusMessage::VidDisperseMsg(proposal) => {
                                HotShotEvent::VidDisperseRecv(proposal, sender)
                            }
                        },
                    };
                    // TODO (Keyao benchmarking) Update these event variants (similar to the
                    // `TransactionsRecv` event) so we can send one event for a vector of messages.
                    // <https://github.com/EspressoSystems/HotShot/issues/1428>
                    self.event_stream.broadcast(event).await;
                }
                MessageKind::Data(message) => match message {
                    hotshot_types::message::DataMessage::SubmitTransaction(transaction, _) => {
                        transactions.push(transaction);
                    }
                },
            };
        }
        if !transactions.is_empty() {
            self.event_stream
                .broadcast(HotShotEvent::TransactionsRecv(transactions))
                .await;
        }
    }
}

/// network event task state
pub struct NetworkEventTaskState<TYPES: NodeType, COMMCHANNEL: CommunicationChannel<TYPES>> {
    /// comm channel
    pub channel: COMMCHANNEL,
    /// view number
    pub view: TYPES::Time,
    /// membership for the channel
    pub membership: TYPES::Membership,
    // TODO ED Need to add exchange so we can get the recipient key and our own key?
}

impl<TYPES: NodeType, COMMCHANNEL: CommunicationChannel<TYPES>> TaskState
    for NetworkEventTaskState<TYPES, COMMCHANNEL>
{
    type Event = HotShotEvent<TYPES>;

    type Result = HotShotTaskCompleted;

    async fn handle_event(
        event: Self::Event,
        task: &mut Task<Self>,
    ) -> Option<HotShotTaskCompleted> {
        let membership = task.state_mut().membership.clone();
        task.state_mut().handle_event(event, &membership).await
    }

    fn should_shutdown(event: &Self::Event) -> bool {
        matches!(event, HotShotEvent::Shutdown)
    }

    fn filter(_event: &Self::Event) -> bool {
        // default doesn't filter
        false
    }

    fn shutdown(&mut self) -> impl std::future::Future<Output = ()> + Send {
        async {}
    }
}

impl<TYPES: NodeType, COMMCHANNEL: CommunicationChannel<TYPES>>
    NetworkEventTaskState<TYPES, COMMCHANNEL>
{
    /// Handle the given event.
    ///
    /// Returns the completion status.
    /// # Panics
    /// Panic sif a direct message event is received with no recipient
    #[allow(clippy::too_many_lines)] // TODO https://github.com/EspressoSystems/HotShot/issues/1704
    #[instrument(skip_all, fields(view = *self.view), name = "Network Task", level = "error")]

    pub async fn handle_event(
        &mut self,
        event: HotShotEvent<TYPES>,
        membership: &TYPES::Membership,
    ) -> Option<HotShotTaskCompleted> {
        let (sender, message_kind, transmit_type, recipient) = match event.clone() {
            HotShotEvent::QuorumProposalSend(proposal, sender) => (
                sender,
                MessageKind::<TYPES>::from_consensus_message(SequencingMessage(Left(
                    GeneralConsensusMessage::Proposal(proposal),
                ))),
                TransmitType::Broadcast,
                None,
            ),

            // ED Each network task is subscribed to all these message types.  Need filters per network task
            HotShotEvent::QuorumVoteSend(vote) => (
                vote.get_signing_key(),
                MessageKind::<TYPES>::from_consensus_message(SequencingMessage(Left(
                    GeneralConsensusMessage::Vote(vote.clone()),
                ))),
                TransmitType::Direct,
                Some(membership.get_leader(vote.get_view_number() + 1)),
            ),
            HotShotEvent::VidDisperseSend(proposal, sender) => (
                sender,
                MessageKind::<TYPES>::from_consensus_message(SequencingMessage(Right(
                    CommitteeConsensusMessage::VidDisperseMsg(proposal),
                ))), // TODO not a CommitteeConsensusMessage https://github.com/EspressoSystems/HotShot/issues/1696
                TransmitType::Broadcast, // TODO not a broadcast https://github.com/EspressoSystems/HotShot/issues/1696
                None,
            ),
            HotShotEvent::DAProposalSend(proposal, sender) => (
                sender,
                MessageKind::<TYPES>::from_consensus_message(SequencingMessage(Right(
                    CommitteeConsensusMessage::DAProposal(proposal),
                ))),
                TransmitType::Broadcast,
                None,
            ),
            HotShotEvent::DAVoteSend(vote) => (
                vote.get_signing_key(),
                MessageKind::<TYPES>::from_consensus_message(SequencingMessage(Right(
                    CommitteeConsensusMessage::DAVote(vote.clone()),
                ))),
                TransmitType::Direct,
                Some(membership.get_leader(vote.get_view_number())),
            ),
            // ED NOTE: This needs to be broadcasted to all nodes, not just ones on the DA committee
            HotShotEvent::DACSend(certificate, sender) => (
                sender,
                MessageKind::<TYPES>::from_consensus_message(SequencingMessage(Right(
                    CommitteeConsensusMessage::DACertificate(certificate),
                ))),
                TransmitType::Broadcast,
                None,
            ),
            HotShotEvent::ViewSyncPreCommitVoteSend(vote) => (
                vote.get_signing_key(),
                MessageKind::<TYPES>::from_consensus_message(SequencingMessage(Left(
                    GeneralConsensusMessage::ViewSyncPreCommitVote(vote.clone()),
                ))),
                TransmitType::Direct,
                Some(membership.get_leader(vote.get_view_number() + vote.get_data().relay)),
            ),
            HotShotEvent::ViewSyncCommitVoteSend(vote) => (
                vote.get_signing_key(),
                MessageKind::<TYPES>::from_consensus_message(SequencingMessage(Left(
                    GeneralConsensusMessage::ViewSyncCommitVote(vote.clone()),
                ))),
                TransmitType::Direct,
                Some(membership.get_leader(vote.get_view_number() + vote.get_data().relay)),
            ),
            HotShotEvent::ViewSyncFinalizeVoteSend(vote) => (
                vote.get_signing_key(),
                MessageKind::<TYPES>::from_consensus_message(SequencingMessage(Left(
                    GeneralConsensusMessage::ViewSyncFinalizeVote(vote.clone()),
                ))),
                TransmitType::Direct,
                Some(membership.get_leader(vote.get_view_number() + vote.get_data().relay)),
            ),
            HotShotEvent::ViewSyncPreCommitCertificate2Send(certificate, sender) => (
                sender,
                MessageKind::<TYPES>::from_consensus_message(SequencingMessage(Left(
                    GeneralConsensusMessage::ViewSyncPreCommitCertificate(certificate),
                ))),
                TransmitType::Broadcast,
                None,
            ),
            HotShotEvent::ViewSyncCommitCertificate2Send(certificate, sender) => (
                sender,
                MessageKind::<TYPES>::from_consensus_message(SequencingMessage(Left(
                    GeneralConsensusMessage::ViewSyncCommitCertificate(certificate),
                ))),
                TransmitType::Broadcast,
                None,
            ),

            HotShotEvent::ViewSyncFinalizeCertificate2Send(certificate, sender) => (
                sender,
                MessageKind::<TYPES>::from_consensus_message(SequencingMessage(Left(
                    GeneralConsensusMessage::ViewSyncFinalizeCertificate(certificate),
                ))),
                TransmitType::Broadcast,
                None,
            ),
            HotShotEvent::TimeoutVoteSend(vote) => (
                vote.get_signing_key(),
                MessageKind::<TYPES>::from_consensus_message(SequencingMessage(Left(
                    GeneralConsensusMessage::TimeoutVote(vote.clone()),
                ))),
                TransmitType::Direct,
                Some(membership.get_leader(vote.get_view_number() + 1)),
            ),
            HotShotEvent::ViewChange(view) => {
                self.view = view;
                return None;
            }
            HotShotEvent::Shutdown => {
                error!("Networking task shutting down");
                return Some(HotShotTaskCompleted);
            }
            event => {
                error!("Receieved unexpected message in network task {:?}", event);
                return None;
            }
        };
        let message = Message {
            version: PROGRAM_PROTOCOL_VERSION,
            sender,
            kind: message_kind,
        };
        let transmit_result = match transmit_type {
            TransmitType::Direct => {
                self.channel
                    .direct_message(message, recipient.unwrap())
                    .await
            }
            TransmitType::Broadcast => self.channel.broadcast_message(message, membership).await,
        };

        match transmit_result {
            Ok(()) => {}
            Err(e) => error!("Failed to send message from network task: {:?}", e),
        }

        None
    }
}
