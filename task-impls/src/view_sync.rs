use crate::events::SequencingHotShotEvent;
use async_compatibility_layer::art::async_sleep;
use async_compatibility_layer::art::async_spawn;
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
use hotshot_types::traits::state::ConsensusTime;
use hotshot_types::vote::ViewSyncData;
use hotshot_types::vote::ViewSyncVote;
use hotshot_types::vote::VoteAccumulator;
use snafu::Snafu;
use std::collections::HashMap;
use std::ops::Deref;
use std::time::Duration;
use std::{marker::PhantomData, sync::Arc};
use tracing::{error, info, warn};

#[derive(PartialEq, PartialOrd)]
pub enum LastSeenViewSyncCeritificate {
    None,
    PreCommit,
    Commit,
    // TODO ED We shouldn't ever reach this stage because this means the protocol is finished
    Finalize,
}

#[derive(Snafu, Debug)]
pub struct ViewSyncTaskError {}
impl TaskErr for ViewSyncTaskError {}

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

    current_replica_task: Option<TYPES::Time>,
    current_relay_task: Option<TYPES::Time>,
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
    pub phase: LastSeenViewSyncCeritificate,
    pub relay: u64,
    pub finalized: bool,

    pub exchange: Arc<ViewSyncEx<TYPES, I>>,

    pub api: A,

    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
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
    pub accumulator: Either<
        VoteAccumulator<TYPES::VoteTokenType, ViewSyncData<TYPES>>,
        ViewSyncCertificate<TYPES>,
    >,
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
                        let (certificate_internal, last_seen_certificate) = match certificate {
                            ViewSyncCertificate::PreCommit(certificate_internal) => (
                                certificate_internal,
                                LastSeenViewSyncCeritificate::PreCommit,
                            ),
                            ViewSyncCertificate::Commit(certificate_internal) => {
                                (certificate_internal, LastSeenViewSyncCeritificate::Commit)
                            }
                            ViewSyncCertificate::Finalize(certificate_internal) => {
                                (certificate_internal, LastSeenViewSyncCeritificate::Finalize)
                            }
                        };

                        // TODO ED Perhaps should keep a map of replica task to view number? But seems wasteful.
                        // Either way, seems like there is perhaps a better way to structure this part
                        // TODO ED This unwrap will fail? Does Rust only execute the first one?
                        // Start a new view sync replica task

                        // This certificate is old, we can throw it away
                        // If next view = cert round, then that means we should already have a task running for it
                        if self.next_view >= certificate_internal.round {
                            if self.current_replica_task.is_none() {
                                // If this panics then we need to double check the logic above, should never panic
                                panic!()
                            }
                            return;
                        }

                        if self.current_replica_task.is_none()
                            || self.current_replica_task.unwrap() < certificate_internal.round
                        {
                            if !self.exchange.is_valid_view_sync_cert() {
                                return;
                            }

                            self.next_view = certificate_internal.round;
                            self.current_replica_task = Some(self.next_view);
                            // TODO ED When we receive view sync finished event set this to None

                            let replica_state = ViewSyncReplicaTaskState {
                                phantom: PhantomData,
                                current_view: self.current_view,
                                next_view: certificate_internal.round,
                                relay: certificate_internal.relay,
                                finalized: false,
                                phase: last_seen_certificate,
                                exchange: self.exchange.clone(),
                                api: self.api.clone(),
                                event_stream: self.event_stream.clone(),
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

                            // Send vote for the cert we just received
                            // TODO ED Change to correct vote message being created below
                            let vote = self.exchange.create_finalize_message::<I>();
                            self.api
                                    .send_direct_message::<QuorumProposalType<TYPES, I>, ViewSyncVote<TYPES>>(
                                        self.exchange.get_leader(
                                            self.next_view + certificate_internal.relay,
                                        ),
                                        vote,
                                    )
                                    .await;

                            // TODO ED Pass along send error from above somewhere
                            // TODO ED Also send to first relay
                        }
                    }
                    ViewSyncMessageType::Vote(vote) => {
                        // TODO ED If task doesn't exist, make it (and check that it is for this relay)

                        let vote_internal = match vote {
                            ViewSyncVote::PreCommit(vote_internal) => vote_internal,
                            ViewSyncVote::Commit(vote_internal) => todo!(),
                            ViewSyncVote::Finalize(vote_internal) => todo!(),
                        };

                        // TODO ED Make another function for this that we can change later
                        if !self.exchange.is_leader(vote_internal.round /* + vote.relay */ ) {
                            return
                        }
                        

                        if self.current_relay_task.is_none() || self.current_relay_task.unwrap() < vote_internal.round{

                            self.current_relay_task = Some(vote_internal.round);

                            let view_sync_data = ViewSyncData::<TYPES> {
                                round: vote_internal.round,
                                relay: self
                                    .exchange
                                    // TODO ED Add relay to below, just use our own public key instead of get leader function
                                    .get_leader(vote_internal.round)
                                    .to_bytes(),
                            }
                            .commit();

                            let mut accumulator = VoteAccumulator {
                                total_vote_outcomes: HashMap::new(),
                                yes_vote_outcomes: HashMap::new(),
                                no_vote_outcomes: HashMap::new(),
                                success_threshold: self.exchange.success_threshold(),
                                failure_threshold: self.exchange.failure_threshold(),
                            };

                            let mut accumulator = self.exchange.accumulate_vote(
                                &vote_internal.signature.0,
                                &vote_internal.signature.1,
                                view_sync_data,
                                vote_internal.vote_data,
                                vote_internal.vote_token.clone(),
                                vote_internal.round,
                                accumulator,
                            );

                            // TODO ED There is a better way to structure this code above and below, put into own function
                            match accumulator {
                                Either::Left(acc) => {
                                    accumulator = Either::Left(acc);
                                }
                                Either::Right(qc) => {
                                    // TODO ED Need to send out next certificate
                                    return ;
                                }
                            }

                            let relay_state = ViewSyncRelayTaskState {
                                phantom: PhantomData,
                                exchange: self.exchange.clone(),
                                accumulator,
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
                // TODO ED Don't call new twice
                if self.current_view < TYPES::Time::new(*new_view) {
                    self.current_view = TYPES::Time::new(*new_view)
                }
            }
            // TODO ED Spawn task to start NK20 protocol (if two timeouts have happened)
            // TODO ED Need to add view number to timeout event
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
                            if self.phase == LastSeenViewSyncCeritificate::PreCommit {
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
                                    // Update relay index if needed
                                    if certificate_internal.relay > self.relay {
                                        self.relay = certificate_internal.relay;
                                    }

                                    let vote = self.exchange.create_finalize_message::<I>();
                                    self.api
                                        .send_direct_message::<QuorumProposalType<TYPES, I>, ViewSyncVote<TYPES>>(
                                            self.exchange.get_leader(
                                                self.next_view + certificate_internal.relay,
                                            ),
                                            vote,
                                        )
                                        .await;

                                    // TODO ED Also send to first relay
                                }

                                // Send ViewChange event to event stream
                                self.event_stream
                                    .publish(SequencingHotShotEvent::ViewChange(ViewNumber::new(
                                        *self.next_view,
                                    )))
                                    .await;

                                async_spawn({
                                    // TODO ED Get actual timeout
                                    let stream = self.event_stream.clone();
                                    async move {
                                        async_sleep(Duration::from_millis(10000)).await;
                                        // TODO ED Needs to know which view number we are timing out?
                                        stream
                                            .publish(SequencingHotShotEvent::ViewSyncTimeout(
                                                ViewNumber::new(*self.next_view),
                                                self.relay,
                                            ))
                                            .await;
                                    }
                                });

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
                    // TODO ED Check if valid vote
                    todo!()
                }
            },
            _ => todo!(),
        }
        return (None, self);
    }
}
