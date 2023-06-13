use crate::{
    event_stream::{ChannelStream, EventStream},
    events::SequencingHotShotEvent,
    task::FilterEvent,
};
use async_compatibility_layer::channel::UnboundedStream;
use either::Either::{self, Left, Right};
use futures::StreamExt;
use hotshot_types::message::Message;
use hotshot_types::message::{CommitteeConsensusMessage, SequencingMessage};
use hotshot_types::{
    data::{ProposalType, SequencingLeaf, ViewNumber},
    message::{GeneralConsensusMessage, MessageKind},
    traits::{
        consensus_type::sequencing_consensus::SequencingConsensus,
        election::Membership,
        network::{CommunicationChannel, TransmitType},
        node_implementation::{NodeImplementation, NodeType},
        signature_key::EncodedSignature,
    },
    vote::VoteType,
};
use std::{marker::PhantomData, sync::Arc};

struct NetworkTask<
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
    channel: COMMCHANNEL,
    events: UnboundedStream<SequencingHotShotEvent<TYPES, I>>,
    event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    view: ViewNumber,
    phantom: PhantomData<(TYPES, Message<TYPES, I>, PROPOSAL, VOTE, MEMBERSHIP)>,
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus, SignatureKey = EncodedSignature>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
        COMMCHANNEL: CommunicationChannel<TYPES, Message<TYPES, I>, PROPOSAL, VOTE, MEMBERSHIP>,
    > NetworkTask<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP, COMMCHANNEL>
{
    /// Handle the given event and return whether to keep running.
    async fn handle_event(
        &mut self,
        event: SequencingHotShotEvent<TYPES, I>,
        membership: MEMBERSHIP,
    ) -> bool {
        let (consensus_message, signature) = match event {
            SequencingHotShotEvent::QuorumProposalSend(proposal) => (
                SequencingMessage(Left(GeneralConsensusMessage::Proposal(proposal.clone()))),
                proposal.signature.clone(),
            ),
            SequencingHotShotEvent::QuorumVoteSend(vote) => (
                SequencingMessage(Left(GeneralConsensusMessage::Vote(vote.clone()))),
                vote.signature().clone(),
            ),
            SequencingHotShotEvent::DAProposalSend(proposal) => (
                SequencingMessage(Right(CommitteeConsensusMessage::DAProposal(
                    proposal.clone(),
                ))),
                proposal.signature.clone(),
            ),
            SequencingHotShotEvent::DAVoteSend(vote) => (
                SequencingMessage(Right(CommitteeConsensusMessage::DAVote(vote.clone()))),
                vote.signature.1.clone(),
            ),
            SequencingHotShotEvent::ViewChange(view) => {
                self.view = view;
                return true;
            }
            SequencingHotShotEvent::Shutdown => {
                self.channel.shut_down().await;
                return false;
            }
            _ => {
                return true;
            }
        };
        let message_kind =
            MessageKind::<SequencingConsensus, TYPES, I>::from_consensus_message(consensus_message);
        let message = Message {
            sender: signature,
            kind: message_kind,
            _phantom: PhantomData,
        };
        self.channel
            .broadcast_message(message, &membership)
            .await
            .expect("Failed to broadcast message");
        return true;
    }

    /// Filter network event.
    fn filter(event: &SequencingHotShotEvent<TYPES, I>) -> bool {
        match event {
            SequencingHotShotEvent::QuorumProposalSend(_)
            | SequencingHotShotEvent::QuorumVoteSend(_)
            | SequencingHotShotEvent::DAProposalSend(_)
            | SequencingHotShotEvent::DAVoteSend(_)
            | SequencingHotShotEvent::Shutdown
            | SequencingHotShotEvent::ViewChange(_) => true,
            _ => false,
        }
    }

    /// Subscribe to network events.
    async fn subscribe(&mut self, event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>) {
        self.events = event_stream
            .subscribe(FilterEvent(Arc::new(Self::filter)))
            .await
            .0
    }

    /// Run when spawning the network tasks.
    async fn run(&mut self, transmit_type: TransmitType, membership: MEMBERSHIP) {
        let messages = self
            .channel
            .recv_msgs(transmit_type)
            .await
            .expect("Failed to receive message");
        for m in messages {
            let event = match m.kind {
                MessageKind::Consensus(consensus_message) => match consensus_message.0 {
                    Either::Left(general_message) => match general_message {
                        GeneralConsensusMessage::Proposal(proposal) => {
                            SequencingHotShotEvent::QuorumProposalRecv(
                                proposal.clone(),
                                proposal.signature,
                            )
                        }
                        GeneralConsensusMessage::Vote(vote) => {
                            SequencingHotShotEvent::QuorumVoteRecv(vote.clone(), vote.signature())
                        }
                        _ => panic!("Got unexpected message type in network task!"),
                    },
                    Either::Right(committee_message) => match committee_message {
                        CommitteeConsensusMessage::DAProposal(proposal) => {
                            SequencingHotShotEvent::DAProposalRecv(
                                proposal.clone(),
                                proposal.signature,
                            )
                        }
                        CommitteeConsensusMessage::DAVote(vote) => {
                            SequencingHotShotEvent::DAVoteRecv(vote.clone(), vote.signature.1)
                        }
                    },
                },
                MessageKind::Data(_) => {
                    panic!("Got unexpected message type in network task!");
                }
                MessageKind::_Unreachable(_) => unimplemented!(),
            };
            self.event_stream.publish(event).await;
        }
        let mut running = true;
        while running {
            let event = self.events.next().await.expect("No event");
            running = self.handle_event(event, membership.clone()).await;
        }
    }
}
