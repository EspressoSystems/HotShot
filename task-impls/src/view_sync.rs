use crate::events::SequencingHotShotEvent;
use async_compatibility_layer::channel::UnboundedStream;
#[cfg(feature = "async-std-executor")]
use async_std::task::JoinHandle;
use commit::Committable;
use either::Either::{self, Left, Right};
use futures::FutureExt;
use futures::StreamExt;
use hotshot_consensus::SequencingConsensusApi;
use hotshot_task::task::HandleEvent;
use hotshot_task::task::HotShotTaskCompleted;
use hotshot_task::task_impls::TaskBuilder;
use hotshot_task::{
    event_stream::{ChannelStream, EventStream},
    task::{FilterEvent, TaskErr, TS},
    task_impls::HSTWithEvent,
};
use hotshot_types::certificate::ViewSyncCertificate;
use hotshot_types::data::QuorumProposal;
use hotshot_types::data::SequencingLeaf;
use hotshot_types::data::ViewNumber;
use hotshot_types::message::Message;
use hotshot_types::message::Proposal;
use hotshot_types::message::SequencingMessage;
use hotshot_types::message::ViewSyncMessageType;
use hotshot_types::traits::consensus_type::sequencing_consensus::SequencingConsensus;
use hotshot_types::traits::election::ConsensusExchange;
use hotshot_types::traits::election::ViewSyncExchangeType;
use hotshot_types::traits::election::ViewSyncVoteData;
use hotshot_types::traits::network::CommunicationChannel;
use hotshot_types::traits::node_implementation::NodeImplementation;
use hotshot_types::traits::node_implementation::NodeType;
use hotshot_types::traits::node_implementation::QuorumProposalType;
use hotshot_types::traits::node_implementation::SequencingExchangesType;
use hotshot_types::traits::node_implementation::ViewSyncEx;
use hotshot_types::traits::signature_key::SignatureKey;
use hotshot_types::vote::ViewSyncData;
use hotshot_types::vote::ViewSyncVote;
use snafu::Snafu;
use std::ops::Deref;
use std::{marker::PhantomData, sync::Arc};
use tracing::{error, info, warn};

/// Represents the latest certificate we have seen
/// i.e. if we have seen a Commit certificate we are in the commit stage
#[derive(PartialEq, PartialOrd)]
pub enum ViewSyncNK20Stage {
    None,
    PreCommit,
    Commit,
    // TODO ED We shouldn't ever reach this stage because this means the protocol is finished
    Finalize,
}

#[derive(Snafu, Debug)]
pub struct ViewSyncTaskError {}
impl TaskErr for ViewSyncTaskError {}

// TODO ED Implement TS trait and Error for this struct
pub struct ViewSyncTaskState<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
    >,
    A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I>
        + std::fmt::Debug
        + 'static
        + std::clone::Clone,
> where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    ViewSyncEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = ViewSyncCertificate<TYPES>,
        Commitment = ViewSyncData<TYPES>,
    >,
{
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    pub filtered_event_stream: UnboundedStream<SequencingHotShotEvent<TYPES, I>>,

    pub current_view: TYPES::Time,
    pub next_view: TYPES::Time,

    // TODO ED Should also have stream to send shutdown
    current_replica_task: Option<ViewNumber>,
    current_relay_task: Option<ViewNumber>,

    pub exchange: Arc<ViewSyncEx<TYPES, I>>,

    pub api: A,
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
        A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I>
            + std::fmt::Debug
            + 'static
            + std::clone::Clone,
    > TS for ViewSyncTaskState<TYPES, I, A>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    ViewSyncEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = ViewSyncCertificate<TYPES>,
        Commitment = ViewSyncData<TYPES>,
    >,
{
}

pub type ViewSyncTaskStateTypes<TYPES, I, A> = HSTWithEvent<
    ViewSyncTaskError,
    SequencingHotShotEvent<TYPES, I>,
    ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    ViewSyncTaskState<TYPES, I, A>,
>;

pub struct ViewSyncReplicaTaskState<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
    >,
    A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + std::fmt::Debug + 'static,
> where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    ViewSyncEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = ViewSyncCertificate<TYPES>,
        Commitment = ViewSyncData<TYPES>,
    >,
{
    phantom: PhantomData<(TYPES, I)>,
    pub current_view: TYPES::Time,
    pub next_view: TYPES::Time,
    pub phase: ViewSyncNK20Stage,

    pub exchange: Arc<ViewSyncEx<TYPES, I>>,

    pub api: A,
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
        A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + std::fmt::Debug + 'static,
    > TS for ViewSyncReplicaTaskState<TYPES, I, A>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    ViewSyncEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = ViewSyncCertificate<TYPES>,
        Commitment = ViewSyncData<TYPES>,
    >,
{
}

pub type ViewSyncReplicaTaskStateTypes<TYPES, I, A> = HSTWithEvent<
    ViewSyncTaskError,
    SequencingHotShotEvent<TYPES, I>,
    ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    ViewSyncReplicaTaskState<TYPES, I, A>,
>;

pub struct ViewSyncRelayTaskState<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
    >,
> {
    phantom: PhantomData<(TYPES, I)>,
    pub exchange: Arc<ViewSyncEx<TYPES, I>>,
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
    > TS for ViewSyncRelayTaskState<TYPES, I>
{
}

pub type ViewSyncRelayTaskStateTypes<TYPES, I> = HSTWithEvent<
    ViewSyncTaskError,
    SequencingHotShotEvent<TYPES, I>,
    ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    ViewSyncRelayTaskState<TYPES, I>,
>;

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
        A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I>
            + std::fmt::Debug
            + 'static
            + std::clone::Clone,
    > ViewSyncTaskState<TYPES, I, A>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    ViewSyncEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = ViewSyncCertificate<TYPES>,
        Commitment = ViewSyncData<TYPES>,
    >,
{
    pub async fn handle_event(&mut self, event: SequencingHotShotEvent<TYPES, I>) {
        match event {
            SequencingHotShotEvent::ViewSyncMessage(message) => {
                match message {
                    ViewSyncMessageType::Certificate(certificate) => {
                        // TODO ED If task doesn't already exist, make it
                        // Don't want to create a new task if one is already running, and several different
                        // view sync certificates (1 for each phase) could trigger us to create this
                        // TODO ED Check which view it is for
                        // TODO ED Check if the cert is for an actual next view that is higher than we have now
                        // TODO ED Don't spawn if none exists and it is a finalize?
                        // TODO ED Peek on stream?  Doesn't seem like it will work here

                        if self.current_replica_task.is_none() {
                            // TODO ED Validate certificate/update certificate so it can be matched based on stage
                            // let phase = match certificate {

                            // };

                            let replica_state = ViewSyncReplicaTaskState {
                                phantom: PhantomData,
                                current_view: self.current_view,
                                next_view: self.next_view,
                                // TODO ED Actually put the correct stage
                                phase: ViewSyncNK20Stage::PreCommit,
                                exchange: self.exchange.clone(),
                                api: self.api.clone(),
                            };
                            let name = format!("View Sync Replica Task: Attempting to enter view {:?} from view {:?}", self.next_view, self.current_view);

                            // TODO ED Passing in mut state seems to make more sense than a separate function not impled on the state?
                            let replica_handle_event = HandleEvent(Arc::new(
                                move |event, mut state: ViewSyncReplicaTaskState<TYPES, I, A>| {
                                    async move { state.handle_event(event).await }.boxed()
                                },
                            ));

                            let filter = FilterEvent::default();
                            let _builder =
                                TaskBuilder::<ViewSyncReplicaTaskStateTypes<TYPES, I, A>>::new(
                                    name,
                                )
                                .register_event_stream(self.event_stream.clone(), filter)
                                .await
                                .register_state(replica_state)
                                .register_event_handler(replica_handle_event);
                        }
                    }
                    ViewSyncMessageType::Vote(vote) => {
                        // TODO ED If task doesn't exist, make it (and check that it is for this relay)

                        if self.current_relay_task.is_none() {
                            let relay_state = ViewSyncRelayTaskState {
                                phantom: PhantomData,
                                exchange: self.exchange.clone(),
                            };
                            let name = format!("View Sync Relay Task: Attempting to enter view {:?} from view {:?}", self.next_view, self.current_view);

                            let relay_handle_event = HandleEvent(Arc::new(
                                move |event, mut state: ViewSyncRelayTaskState<TYPES, I>| {
                                    async move { state.handle_event(event).await }.boxed()
                                },
                            ));

                            let filter = FilterEvent::default();
                            let _builder =
                                TaskBuilder::<ViewSyncRelayTaskStateTypes<TYPES, I>>::new(name)
                                    .register_event_stream(self.event_stream.clone(), filter)
                                    .await
                                    .register_state(relay_state)
                                    .register_event_handler(relay_handle_event);
                        }
                    }
                }
            }
            SequencingHotShotEvent::ViewChange(new_view) => {
                // if self.current_view < new_view {
                //     self.current_view = new_view
                // }
                todo!()
            }
            // TODO ED Spawn task to start NK20 protocol
            SequencingHotShotEvent::Timeout => todo!(),
            _ => todo!(),
        };
        return;
    }

    // Resets state once view sync has completed
    fn clear_state(&mut self) {
        todo!()
    }

    /// Filter view sync related events.
    fn filter(event: &SequencingHotShotEvent<TYPES, I>) -> bool {
        match event {
            SequencingHotShotEvent::QuorumProposalSend(_)
            | SequencingHotShotEvent::ViewSyncMessage(_)
            | SequencingHotShotEvent::Shutdown
            | SequencingHotShotEvent::ViewChange(_) => true,
            _ => false,
        }
    }

    /// Subscribe to view sync events.
    async fn subscribe(&mut self, event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>) {
        self.filtered_event_stream = event_stream
            .subscribe(FilterEvent(Arc::new(Self::filter)))
            .await
            .0
    }
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
        A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + std::fmt::Debug + 'static,
    > ViewSyncReplicaTaskState<TYPES, I, A>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    ViewSyncEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = ViewSyncCertificate<TYPES>,
        Commitment = ViewSyncData<TYPES>,
    >,
{
    pub async fn handle_event(
        mut self,
        event: SequencingHotShotEvent<TYPES, I>,
    ) -> (
        std::option::Option<HotShotTaskCompleted>,
        ViewSyncReplicaTaskState<TYPES, I, A>,
    ) {
        match event {
            SequencingHotShotEvent::ViewSyncMessage(message) => match message {
                ViewSyncMessageType::Certificate(certificate) => {
                    match certificate {
                        ViewSyncCertificate::PreCommit(certificate_internal) => {
                            // Check cert for validity
                            // send vote
                            todo!()
                        }
                        ViewSyncCertificate::Commit(certificate_internal) => {
                            if self.phase == ViewSyncNK20Stage::PreCommit {
                                // TODO ED This check will fail because we have an extra wrapping of an enum, should maybe add diff certificate types without enum wrapping, also will fail on precommit cert since that only needs f+1 votes, could maybe just make separate is_valid_vs_cert function
                                if self.exchange.is_valid_cert(
                                    &ViewSyncCertificate::Commit(certificate_internal.clone()),
                                    ViewSyncData {
                                        round: self.next_view,
                                        relay: self
                                            .exchange
                                            .get_leader(self.next_view + certificate_internal.relay)
                                            .to_bytes(),
                                    }
                                    .commit(),
                                ) {
                                    let vote = self.exchange.create_finalize_message::<I>();
                                    self.api
                                        .send_direct_message::<QuorumProposalType<TYPES, I>, ViewSyncVote<TYPES>>(
                                            self.exchange.get_leader(
                                                self.next_view + certificate_internal.relay,
                                            ),
                                            vote,
                                        )
                                        .await;
                                }

                                todo!()
                            }
                        }
                        ViewSyncCertificate::Finalize(certificate_internal) => {
                            todo!()
                        }
                    }
                    todo!()
                }
                ViewSyncMessageType::Vote(vote) => {
                    todo!()
                }
            },
            // TODO ED Add view sync timeout event
            _ => todo!(),
        }
        return (None, self);
    }
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
    > ViewSyncRelayTaskState<TYPES, I>
{
    pub async fn handle_event(
        mut self,
        event: SequencingHotShotEvent<TYPES, I>,
    ) -> (
        std::option::Option<HotShotTaskCompleted>,
        ViewSyncRelayTaskState<TYPES, I>,
    ) {
        match event {
            SequencingHotShotEvent::ViewSyncMessage(message) => match message {
                ViewSyncMessageType::Certificate(certificate) => {
                    todo!()
                }
                ViewSyncMessageType::Vote(vote) => {
                    todo!()
                }
            },
            _ => todo!(),
        }
        return (None, self);
    }
}

// self.timeout_task = async_spawn({
//     // let next_view_timeout = hotshot.inner.config.next_view_timeout;
//     // let next_view_timeout = next_view_timeout;
//     // let hotshot: HotShot<TYPES::ConsensusType, TYPES, I> = hotshot.clone();
//     // TODO(bf): get the real timeout from the config.
//     let stream = self.event_stream.clone();
//     async move {
//         async_sleep(Duration::from_millis(10000)).await;
//         // TODO ED Needs to know which view number we are timing out?
//         stream.publish(SequencingHotShotEvent::Timeout).await;
//     }
// });
