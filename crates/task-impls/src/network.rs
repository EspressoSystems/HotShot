use crate::events::SequencingHotShotEvent;
use either::Either::{self, Left, Right};
use hotshot_task::{
    event_stream::{ChannelStream, EventStream},
    task::{FilterEvent, HotShotTaskCompleted, TS},
    task_impls::{HSTWithEvent, HSTWithMessage},
    GeneratedStream, Merge,
};
use hotshot_types::{
    data::{ProposalType, SequencingLeaf},
    message::{
        CommitteeConsensusMessage, GeneralConsensusMessage, Message, MessageKind, Messages,
        SequencingMessage,
    },
    traits::{
        election::Membership,
        network::{CommunicationChannel, TransmitType},
        node_implementation::{NodeImplementation, NodeType},
    },
    vote::VoteType,
};
use snafu::Snafu;
use std::{marker::PhantomData, sync::Arc};
use tracing::error;

/// the type of network task
#[derive(Clone, Copy, Debug)]
pub enum NetworkTaskKind {
    /// quorum: the normal "everyone" committee
    Quorum,
    /// da committee
    Committee,
    /// view sync
    ViewSync,
}

/// the network message task state
pub struct NetworkMessageTaskState<
    TYPES: NodeType,
    I: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
    >,
> {
    /// event stream (used for publishing)
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
    > TS for NetworkMessageTaskState<TYPES, I>
{
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
    > NetworkMessageTaskState<TYPES, I>
{
    /// Handle the message.
    pub async fn handle_messages(&mut self, messages: Vec<Message<TYPES, I>>) {
        // We will send only one event for a vector of transactions.
        let mut transactions = Vec::new();
        for message in messages {
            let sender = message.sender;
            match message.kind {
                MessageKind::Consensus(consensus_message) => {
                    let event = match consensus_message.0 {
                        Either::Left(general_message) => match general_message {
                            GeneralConsensusMessage::Proposal(proposal) => {
                                SequencingHotShotEvent::QuorumProposalRecv(proposal.clone(), sender)
                            }
                            GeneralConsensusMessage::Vote(vote) => {
                                SequencingHotShotEvent::QuorumVoteRecv(vote.clone())
                            }
                            GeneralConsensusMessage::ViewSyncVote(view_sync_message) => {
                                SequencingHotShotEvent::ViewSyncVoteRecv(view_sync_message)
                            }
                            GeneralConsensusMessage::ViewSyncCertificate(view_sync_message) => {
                                SequencingHotShotEvent::ViewSyncCertificateRecv(view_sync_message)
                            }
                            GeneralConsensusMessage::InternalTrigger(_) => {
                                error!("Got unexpected message type in network task!");
                                return;
                            }
                        },
                        Either::Right(committee_message) => match committee_message {
                            CommitteeConsensusMessage::DAProposal(proposal) => {
                                SequencingHotShotEvent::DAProposalRecv(proposal.clone(), sender)
                            }
                            CommitteeConsensusMessage::DAVote(vote) => {
                                // error!("DA Vote message recv {:?}", vote.current_view);
                                SequencingHotShotEvent::DAVoteRecv(vote.clone())
                            }
                            CommitteeConsensusMessage::DACertificate(cert) => {
                                // panic!("Recevid DA C! ");
                                SequencingHotShotEvent::DACRecv(cert)
                            }
                            CommitteeConsensusMessage::VidDisperseMsg(proposal) => {
                                SequencingHotShotEvent::VidDisperseRecv(proposal, sender)
                            }
                            CommitteeConsensusMessage::VidVote(vote) => {
                                SequencingHotShotEvent::VidVoteRecv(vote.clone())
                            }
                            CommitteeConsensusMessage::VidCertificate(_cert) => {
                                todo!()
                                // SequencingHotShotEvent::VidCertRecv(cert)
                            }
                        },
                    };
                    // TODO (Keyao benchmarking) Update these event variants (similar to the
                    // `TransactionsRecv` event) so we can send one event for a vector of messages.
                    // <https://github.com/EspressoSystems/HotShot/issues/1428>
                    self.event_stream.publish(event).await;
                }
                MessageKind::Data(message) => match message {
                    hotshot_types::message::DataMessage::SubmitTransaction(transaction, _) => {
                        transactions.push(transaction);
                    }
                },
                MessageKind::_Unreachable(_) => unimplemented!(),
            };
        }
        if !transactions.is_empty() {
            self.event_stream
                .publish(SequencingHotShotEvent::TransactionsRecv(transactions))
                .await;
        }
    }
}

/// network event task state
pub struct NetworkEventTaskState<
    TYPES: NodeType,
    I: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
    >,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
    MEMBERSHIP: Membership<TYPES>,
    COMMCHANNEL: CommunicationChannel<TYPES, Message<TYPES, I>, PROPOSAL, VOTE, MEMBERSHIP>,
> {
    /// comm channel
    pub channel: COMMCHANNEL,
    /// event stream
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    /// view number
    pub view: TYPES::Time,
    /// phantom data
    pub phantom: PhantomData<(PROPOSAL, VOTE, MEMBERSHIP)>,
    // TODO ED Need to add exchange so we can get the recipient key and our own key?
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
        COMMCHANNEL: CommunicationChannel<TYPES, Message<TYPES, I>, PROPOSAL, VOTE, MEMBERSHIP>,
    > TS for NetworkEventTaskState<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP, COMMCHANNEL>
{
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
        COMMCHANNEL: CommunicationChannel<TYPES, Message<TYPES, I>, PROPOSAL, VOTE, MEMBERSHIP>,
    > NetworkEventTaskState<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP, COMMCHANNEL>
{
    /// Handle the given event.
    ///
    /// Returns the completion status.
    /// # Panics
    /// Panic sif a direct message event is received with no recipient
    pub async fn handle_event(
        &mut self,
        event: SequencingHotShotEvent<TYPES, I>,
        membership: &MEMBERSHIP,
    ) -> Option<HotShotTaskCompleted> {
        let (sender, message_kind, transmit_type, recipient) = match event.clone() {
            SequencingHotShotEvent::QuorumProposalSend(proposal, sender) => (
                sender,
                MessageKind::<TYPES, I>::from_consensus_message(SequencingMessage(Left(
                    GeneralConsensusMessage::Proposal(proposal),
                ))),
                TransmitType::Broadcast,
                None,
            ),

            // ED Each network task is subscribed to all these message types.  Need filters per network task
            SequencingHotShotEvent::QuorumVoteSend(vote) => (
                vote.signature_key(),
                MessageKind::<TYPES, I>::from_consensus_message(SequencingMessage(Left(
                    GeneralConsensusMessage::Vote(vote.clone()),
                ))),
                TransmitType::Direct,
                Some(membership.get_leader(vote.current_view() + 1)),
            ),

            // TODO GG `VidDiserpseSend` like `DAProposalSend` for now
            SequencingHotShotEvent::VidDisperseSend(proposal, sender) => (
                sender,
                MessageKind::<TYPES, I>::from_consensus_message(SequencingMessage(Right(
                    CommitteeConsensusMessage::VidDisperseMsg(proposal),
                ))), // TODO GG not a CommitteeConsensusMessage
                TransmitType::Broadcast, // TODO GG broadcast??
                None,
            ),
            SequencingHotShotEvent::DAProposalSend(proposal, sender) => (
                sender,
                MessageKind::<TYPES, I>::from_consensus_message(SequencingMessage(Right(
                    CommitteeConsensusMessage::DAProposal(proposal),
                ))),
                TransmitType::Broadcast,
                None,
            ),
            SequencingHotShotEvent::VidVoteSend(vote) => (
                vote.signature_key(),
                MessageKind::<TYPES, I>::from_consensus_message(SequencingMessage(Right(
                    CommitteeConsensusMessage::VidVote(vote.clone()),
                ))),
                TransmitType::Direct,
                Some(membership.get_leader(vote.current_view)), // TODO GG who is VID "leader"?
            ),
            SequencingHotShotEvent::DAVoteSend(vote) => (
                vote.signature_key(),
                MessageKind::<TYPES, I>::from_consensus_message(SequencingMessage(Right(
                    CommitteeConsensusMessage::DAVote(vote.clone()),
                ))),
                TransmitType::Direct,
                Some(membership.get_leader(vote.current_view)),
            ),
            SequencingHotShotEvent::VidCertSend(certificate, sender) => (
                sender,
                MessageKind::<TYPES, I>::from_consensus_message(SequencingMessage(Right(
                    CommitteeConsensusMessage::VidCertificate(certificate),
                ))),
                TransmitType::Broadcast,
                None,
            ),
            // ED NOTE: This needs to be broadcasted to all nodes, not just ones on the DA committee
            SequencingHotShotEvent::DACSend(certificate, sender) => (
                sender,
                MessageKind::<TYPES, I>::from_consensus_message(SequencingMessage(Right(
                    CommitteeConsensusMessage::DACertificate(certificate),
                ))),
                TransmitType::Broadcast,
                None,
            ),
            SequencingHotShotEvent::ViewSyncCertificateSend(certificate_proposal, sender) => (
                sender,
                MessageKind::<TYPES, I>::from_consensus_message(SequencingMessage(Left(
                    GeneralConsensusMessage::ViewSyncCertificate(certificate_proposal),
                ))),
                TransmitType::Broadcast,
                None,
            ),
            SequencingHotShotEvent::ViewSyncVoteSend(vote) => {
                // error!("Sending view sync vote in network task to relay with index: {:?}", vote.round() + vote.relay());
                (
                    vote.signature_key(),
                    MessageKind::<TYPES, I>::from_consensus_message(SequencingMessage(Left(
                        GeneralConsensusMessage::ViewSyncVote(vote.clone()),
                    ))),
                    TransmitType::Direct,
                    Some(membership.get_leader(vote.round() + vote.relay())),
                )
            }
            SequencingHotShotEvent::ViewChange(view) => {
                self.view = view;
                return None;
            }
            SequencingHotShotEvent::Shutdown => {
                return Some(HotShotTaskCompleted::ShutDown);
            }
            event => {
                error!("Receieved unexpected message in network task {:?}", event);
                return None;
            }
        };

        let message = Message {
            sender,
            kind: message_kind,
            _phantom: PhantomData,
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

    /// network filter
    pub fn filter(task_kind: NetworkTaskKind) -> FilterEvent<SequencingHotShotEvent<TYPES, I>> {
        match task_kind {
            NetworkTaskKind::Quorum => FilterEvent(Arc::new(Self::quorum_filter)),
            NetworkTaskKind::Committee => FilterEvent(Arc::new(Self::committee_filter)),
            NetworkTaskKind::ViewSync => FilterEvent(Arc::new(Self::view_sync_filter)),
        }
    }

    /// quorum filter
    fn quorum_filter(event: &SequencingHotShotEvent<TYPES, I>) -> bool {
        matches!(
            event,
            SequencingHotShotEvent::QuorumProposalSend(_, _)
                | SequencingHotShotEvent::QuorumVoteSend(_)
                | SequencingHotShotEvent::Shutdown
                | SequencingHotShotEvent::DACSend(_, _)
                | SequencingHotShotEvent::VidCertSend(_, _)
                | SequencingHotShotEvent::ViewChange(_)
        )
    }

    /// committee filter
    fn committee_filter(event: &SequencingHotShotEvent<TYPES, I>) -> bool {
        matches!(
            event,
            SequencingHotShotEvent::DAProposalSend(_, _)
                | SequencingHotShotEvent::DAVoteSend(_)
                | SequencingHotShotEvent::Shutdown
                | SequencingHotShotEvent::VidDisperseSend(_, _)
                | SequencingHotShotEvent::VidVoteSend(_)
                | SequencingHotShotEvent::ViewChange(_)
        )
    }

    /// view sync filter
    fn view_sync_filter(event: &SequencingHotShotEvent<TYPES, I>) -> bool {
        matches!(
            event,
            SequencingHotShotEvent::ViewSyncVoteSend(_)
                | SequencingHotShotEvent::ViewSyncCertificateSend(_, _)
                | SequencingHotShotEvent::Shutdown
                | SequencingHotShotEvent::ViewChange(_)
        )
    }
}

/// network error (no errors right now, only stub)
#[derive(Snafu, Debug)]
pub struct NetworkTaskError {}

/// networking message task types
pub type NetworkMessageTaskTypes<TYPES, I> = HSTWithMessage<
    NetworkTaskError,
    Either<Messages<TYPES, I>, Messages<TYPES, I>>,
    // A combination of broadcast and direct streams.
    Merge<GeneratedStream<Messages<TYPES, I>>, GeneratedStream<Messages<TYPES, I>>>,
    NetworkMessageTaskState<TYPES, I>,
>;

/// network event task types
pub type NetworkEventTaskTypes<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP, COMMCHANNEL> = HSTWithEvent<
    NetworkTaskError,
    SequencingHotShotEvent<TYPES, I>,
    ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    NetworkEventTaskState<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP, COMMCHANNEL>,
>;
