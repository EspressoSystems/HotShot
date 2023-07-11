use crate::events::SequencingHotShotEvent;
use either::Either::{self, Left, Right};
use hotshot_task::{
    event_stream::{ChannelStream, EventStream},
    task::{HotShotTaskCompleted, TaskErr, TS},
    task_impls::HSTWithEventAndMessage,
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
use std::marker::PhantomData;
use tracing::error;
use tracing::warn;

pub struct NetworkTaskState<
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
    > TS for NetworkTaskState<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP, COMMCHANNEL>
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
    > NetworkTaskState<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP, COMMCHANNEL>
{
    /// Handle the given message.
    // ED NOTE: Perhaps we should have a different handle event function for each exchange?  Otherwise we are duplicating events like QuorumProposalRecv
    pub async fn handle_message(&mut self, message: Message<TYPES, I>) {
        let sender = message.sender;
        let event = match message.kind {
            MessageKind::Consensus(consensus_message) => match consensus_message.0 {
                Either::Left(general_message) => match general_message {
                    GeneralConsensusMessage::Proposal(proposal) => {
                        // error!("Recved quorum proposal");
                        SequencingHotShotEvent::QuorumProposalRecv(proposal.clone(), sender)
                    }
                    GeneralConsensusMessage::Vote(vote) => {
                        // error!("Recved quorum vote");

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
                        error!("DA Vote message recv {:?}", vote.current_view);
                        SequencingHotShotEvent::DAVoteRecv(vote.clone())
                    }
                    CommitteeConsensusMessage::DACertificate(cert) => {
                        // panic!("Recevid DA C! ");
                        SequencingHotShotEvent::DACRecv(cert)
                    }
                },
            },
            MessageKind::Data(message) => {
                match message {
                    // ED Why do we need the view number in the transaction?
                    hotshot_types::message::DataMessage::SubmitTransaction(
                        transaction,
                        view_number,
                    ) => {
                        SequencingHotShotEvent::TransactionRecv(transaction)
                    }
                }
            }
            MessageKind::_Unreachable(_) => unimplemented!(),
        };
        self.event_stream.publish(event).await;
    }

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
            SequencingHotShotEvent::DACSend(certificate, sender) =>{ 
                    // panic!("Sending DAC!!!!!!!");
                (
                sender,
                MessageKind::<SequencingConsensus, TYPES, I>::from_consensus_message(
                    SequencingMessage(Right(CommitteeConsensusMessage::DACertificate(
                        certificate.clone(),
                    ))),
                ),
                TransmitType::Broadcast,
                None,
            )},
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
            SequencingHotShotEvent::ViewSyncVoteSend(vote) => (
                vote.signature_key(),
                MessageKind::<SequencingConsensus, TYPES, I>::from_consensus_message(
                    SequencingMessage(Left(GeneralConsensusMessage::ViewSyncVote(vote.clone()))),
                ),
                TransmitType::Direct,
                Some(membership.get_leader(vote.round() + vote.relay())),
            ),
            SequencingHotShotEvent::TransactionSend(transaction) => (
                // TODO ED Get our own key
                nll_todo(),
                MessageKind::<SequencingConsensus, TYPES, I>::from(DataMessage::SubmitTransaction(
                    transaction,
                    TYPES::Time::new(*self.view),
                )),
                TransmitType::Broadcast,
                None,
            ),
            SequencingHotShotEvent::ViewChange(view) => {
                self.view = view;
                return None;
            }
            SequencingHotShotEvent::Shutdown => {
                return Some(HotShotTaskCompleted::ShutDown);
            }
            event=> {
                // panic!("Receieved unexpected message in network task {:?}", event);
                return None;
            }
        };

        let message = Message {
            sender,
            kind: message_kind,
            _phantom: PhantomData,
        };
        match transmit_type {
            TransmitType::Direct => self
                .channel
                .direct_message(message, recipient.unwrap())
                .await
                .expect("Failed to direct message"),
            TransmitType::Broadcast => self
                .channel
                .broadcast_message(message, membership)
                .await
                .expect("Failed to broadcast message"),
        }

        return None;
    }

}

#[derive(Snafu, Debug)]
pub struct NetworkTaskError {}
impl TaskErr for NetworkTaskError {}

pub type NetworkTaskTypes<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP, COMMCHANNEL> =
    HSTWithEventAndMessage<
        NetworkTaskError,
        SequencingHotShotEvent<TYPES, I>,
        ChannelStream<SequencingHotShotEvent<TYPES, I>>,
        Either<Messages<TYPES, I>, Messages<TYPES, I>>,
        // A combination of broadcast and direct streams.
        Merge<GeneratedStream<Messages<TYPES, I>>, GeneratedStream<Messages<TYPES, I>>>,
        NetworkTaskState<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP, COMMCHANNEL>,
    >;
