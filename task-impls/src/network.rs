use crate::events::SequencingHotShotEvent;
use either::Either::{self, Left, Right};
use hotshot_task::{
    event_stream::{ChannelStream, EventStream},
    task::{FilterEvent, HotShotTaskCompleted, TaskErr, TS},
    task_impls::{HSTWithEvent, HSTWithMessage},
    GeneratedStream, Merge,
};
use hotshot_types::message::{DataMessage, Message};
use hotshot_types::traits::state::ConsensusTime;
use hotshot_types::{
    data::{ProposalType, SequencingLeaf, ViewNumber},
    message::{GeneralConsensusMessage, MessageKind, Messages},
    traits::{
        consensus_type::sequencing_consensus::SequencingConsensus,
        election::Membership,
        network::{CommunicationChannel, TransmitType},
        node_implementation::{NodeImplementation, NodeType},
        signature_key::EncodedSignature,
    },
    vote::VoteType,
};
use hotshot_types::{
    message::{CommitteeConsensusMessage, SequencingMessage},
    traits::election::SignedCertificate,
};
use nll::nll_todo::nll_todo;
use snafu::Snafu;
use std::{marker::PhantomData, sync::Arc};
use tracing::error;
use tracing::warn;

#[derive(Clone, Copy, Debug)]
pub enum NetworkTaskKind {
    Quorum,
    Committee,
    ViewSync,
}

pub struct NetworkMessageTaskState<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
    >,
> {
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
    > TS for NetworkMessageTaskState<TYPES, I>
{
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
    > NetworkMessageTaskState<TYPES, I>
{
    /// Handle the message.
    pub async fn handle_messages(&mut self, messages: Vec<Message<TYPES, I>>, id: u64) {
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
                            _ => {
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
                        },
                    };
                    // TODO (Keyao benchmarking) Update these event variants (similar to the
                    // `TransactionsRecv` event) so we can send one event for a vector of messages.
                    // <https://github.com/EspressoSystems/HotShot/issues/1428>
                    self.event_stream.publish(event).await;
                }
                MessageKind::Data(message) => match message {
                    hotshot_types::message::DataMessage::SubmitTransaction(transaction, _) => {
                        transactions.push(transaction)
                    }
                },
                MessageKind::_Unreachable(_) => unimplemented!(),
            };
        }
        if !transactions.is_empty() {
            error!("Transactions in network task are {}", transactions.len());
            self.event_stream
                .publish(SequencingHotShotEvent::TransactionsRecv(transactions))
                .await;
        }
    }
}

pub struct NetworkEventTaskState<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
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
    pub channel: COMMCHANNEL,
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    pub view: ViewNumber,
    pub phantom: PhantomData<(PROPOSAL, VOTE, MEMBERSHIP)>,
    // TODO ED Need to add exchange so we can get the recipient key and our own key?
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
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
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
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
    pub async fn handle_event(
        &mut self,
        event: SequencingHotShotEvent<TYPES, I>,
        membership: &MEMBERSHIP,
    ) -> Option<HotShotTaskCompleted> {
        let (sender, message_kind, transmit_type, recipient) = match event {
            SequencingHotShotEvent::QuorumProposalSend(proposal, sender) => (
                sender,
                MessageKind::<SequencingConsensus, TYPES, I>::from_consensus_message(
                    SequencingMessage(Left(GeneralConsensusMessage::Proposal(proposal.clone()))),
                ),
                TransmitType::Broadcast,
                None,
            ),

            // ED Each network task is subscribed to all these message types.  Need filters per network task
            SequencingHotShotEvent::QuorumVoteSend(vote) => (
                vote.signature_key(),
                MessageKind::<SequencingConsensus, TYPES, I>::from_consensus_message(
                    SequencingMessage(Left(GeneralConsensusMessage::Vote(vote.clone()))),
                ),
                TransmitType::Direct,
                Some(membership.get_leader(vote.current_view() + 1)),
            ),

            SequencingHotShotEvent::DAProposalSend(proposal, sender) => (
                sender,
                MessageKind::<SequencingConsensus, TYPES, I>::from_consensus_message(
                    SequencingMessage(Right(CommitteeConsensusMessage::DAProposal(
                        proposal.clone(),
                    ))),
                ),
                TransmitType::Broadcast,
                None,
            ),
            SequencingHotShotEvent::DAVoteSend(vote) => (
                vote.signature_key(),
                MessageKind::<SequencingConsensus, TYPES, I>::from_consensus_message(
                    SequencingMessage(Right(CommitteeConsensusMessage::DAVote(vote.clone()))),
                ),
                TransmitType::Direct,
                Some(membership.get_leader(vote.current_view)),
            ),
            // ED NOTE: This needs to be broadcasted to all nodes, not just ones on the DA committee
            SequencingHotShotEvent::DACSend(certificate, sender) => (
                sender,
                MessageKind::<SequencingConsensus, TYPES, I>::from_consensus_message(
                    SequencingMessage(Right(CommitteeConsensusMessage::DACertificate(
                        certificate.clone(),
                    ))),
                ),
                TransmitType::Broadcast,
                None,
            ),
            SequencingHotShotEvent::ViewSyncCertificateSend(certificate_proposal, sender) => (
                sender,
                MessageKind::<SequencingConsensus, TYPES, I>::from_consensus_message(
                    SequencingMessage(Left(GeneralConsensusMessage::ViewSyncCertificate(
                        certificate_proposal.clone(),
                    ))),
                ),
                TransmitType::Broadcast,
                None,
            ),
            SequencingHotShotEvent::ViewSyncVoteSend(vote) => {
                // error!("Sending view sync vote in network task to relay with index: {:?}", vote.round() + vote.relay());
                (
                    vote.signature_key(),
                    MessageKind::<SequencingConsensus, TYPES, I>::from_consensus_message(
                        SequencingMessage(Left(GeneralConsensusMessage::ViewSyncVote(
                            vote.clone(),
                        ))),
                    ),
                    TransmitType::Direct,
                    Some(membership.get_leader(vote.round() + vote.relay())),
                )
            }
            SequencingHotShotEvent::ViewChange(view) => {
                // only if view actually changes
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

        return None;
    }

    pub fn filter(task_kind: NetworkTaskKind) -> FilterEvent<SequencingHotShotEvent<TYPES, I>> {
        match task_kind {
            NetworkTaskKind::Quorum => FilterEvent(Arc::new(Self::quorum_filter)),
            NetworkTaskKind::Committee => FilterEvent(Arc::new(Self::committee_filter)),
            NetworkTaskKind::ViewSync => FilterEvent(Arc::new(Self::view_sync_filter)),
        }
    }

    fn quorum_filter(event: &SequencingHotShotEvent<TYPES, I>) -> bool {
        match event {
            SequencingHotShotEvent::QuorumProposalSend(_, _)
            | SequencingHotShotEvent::QuorumVoteSend(_)
            | SequencingHotShotEvent::Shutdown
            | SequencingHotShotEvent::DACSend(_, _)
            | SequencingHotShotEvent::ViewChange(_) => true,

            _ => false,
        }
    }

    fn committee_filter(event: &SequencingHotShotEvent<TYPES, I>) -> bool {
        match event {
            SequencingHotShotEvent::DAProposalSend(_, _)
            | SequencingHotShotEvent::DAVoteSend(_)
            | SequencingHotShotEvent::Shutdown
            | SequencingHotShotEvent::ViewChange(_) => true,

            _ => false,
        }
    }

    fn view_sync_filter(event: &SequencingHotShotEvent<TYPES, I>) -> bool {
        match event {
            SequencingHotShotEvent::ViewSyncVoteSend(_)
            | SequencingHotShotEvent::ViewSyncCertificateSend(_, _)
            | SequencingHotShotEvent::Shutdown
            | SequencingHotShotEvent::ViewChange(_) => true,

            _ => false,
        }
    }
}

#[derive(Snafu, Debug)]
pub struct NetworkTaskError {}
impl TaskErr for NetworkTaskError {}

pub type NetworkMessageTaskTypes<TYPES, I> = HSTWithMessage<
    NetworkTaskError,
    Either<Messages<TYPES, I>, Messages<TYPES, I>>,
    // A combination of broadcast and direct streams.
    Merge<GeneratedStream<Messages<TYPES, I>>, GeneratedStream<Messages<TYPES, I>>>,
    NetworkMessageTaskState<TYPES, I>,
>;

pub type NetworkEventTaskTypes<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP, COMMCHANNEL> = HSTWithEvent<
    NetworkTaskError,
    SequencingHotShotEvent<TYPES, I>,
    ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    NetworkEventTaskState<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP, COMMCHANNEL>,
>;