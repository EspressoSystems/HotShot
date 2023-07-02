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
use hotshot_types::traits::election::Membership;
use hotshot_types::traits::election::SignedCertificate;
use hotshot_types::traits::election::VoteData;

use hotshot_types::certificate::ViewSyncCertificate;
use hotshot_types::data::QuorumProposal;
use hotshot_types::data::SequencingLeaf;
use hotshot_types::data::ViewNumber;
use hotshot_types::message::GeneralConsensusMessage;
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

// TODO ED Rename
#[derive(PartialEq, PartialOrd, Clone, Debug)]
pub enum LastSeenViewSyncCeritificate {
    None,
    PreCommit,
    Commit,
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

    // How many timeouts we've seen in a row; is reset upon a successful view change
    pub num_timeouts_tracked: u64,

    // Represents if replica task is running, if relay task is running
    pub task_map: HashMap<TYPES::Time, (bool, bool)>,
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
    pub phantom: PhantomData<(TYPES, I)>,
    pub current_view: TYPES::Time,
    pub next_view: TYPES::Time,
    pub phase: LastSeenViewSyncCeritificate,
    pub relay: u64,
    pub finalized: bool,
    pub sent_view_change_event: bool,

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
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
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
        let event2 = event.clone();
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

                        // Start a new view sync replica task

                        // TODO ED Could make replica.next_view static and immutable

                        // This certificate is old, we can throw it away
                        // If next view = cert round, then that means we should already have a task running for it
                        if self.current_view > certificate_internal.round {
                            // if self.current_replica_task.is_none() {
                            //     // If this panics then we need to double check the logic above, should never panic
                            //     panic!()
                            // }
                            return;
                        }

                        // If we do not already have a task for this view
                        if self.task_map.get(&certificate_internal.round).is_none()
                        // TODO Also need to account for if entry already exists from a relay bound message
                        // || self.current_replica_task.unwrap() < certificate_internal.round
                        {
                            // Need to check cert is valid to avoid attack of someone sending an invalid cert and messing up everyone's view sync
                            // Or can use current view to let multiple tasks runs

                            // TODO ED Don't know if the certificate is valid or not yet...

                            self.task_map
                                .insert(certificate_internal.round, (true, false));
                            // TODO ED Need to GC old entries once we know we don't need them anymore

                            // TODO ED I think this can just be default, make a function
                            // TODO ED Probably don't need current view really
                            let mut replica_state = ViewSyncReplicaTaskState {
                                phantom: PhantomData,
                                current_view: certificate_internal.round,
                                next_view: certificate_internal.round,
                                relay: 0,
                                finalized: false,
                                sent_view_change_event: false,
                                phase: LastSeenViewSyncCeritificate::None,
                                exchange: self.exchange.clone(),
                                api: self.api.clone(),
                                event_stream: self.event_stream.clone(),
                            };

                            // TODO ED Only if returns that replica state should keep running, if is finalized cert then it should stop
                            replica_state = replica_state.handle_event(event2).await.1;

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
                                .register_event_stream(replica_state.event_stream.clone(), filter)
                                .await
                                .register_state(replica_state)
                                .register_event_handler(replica_handle_event);
                        }
                    }
                    ViewSyncMessageType::Vote(vote) => {
                        let vote_internal = match vote {
                            ViewSyncVote::PreCommit(vote_internal) => vote_internal,
                            ViewSyncVote::Commit(vote_internal) => vote_internal,
                            ViewSyncVote::Finalize(vote_internal) => vote_internal,
                        };

                        if self.task_map.get(&vote_internal.round).is_none()
                        // TODO ED Also need to account for if entry already exists from a relay bound message
                        {
                            self.task_map.insert(vote_internal.round, (false, true));

                            // TODO ED Make another function for this that we can change later
                            if !self
                                .exchange
                                .is_leader(vote_internal.round + vote_internal.relay)
                            {
                                return;
                            }

                            let mut accumulator = VoteAccumulator {
                                total_vote_outcomes: HashMap::new(),
                                yes_vote_outcomes: HashMap::new(),
                                no_vote_outcomes: HashMap::new(),
                                success_threshold: self.exchange.success_threshold(),
                                failure_threshold: self.exchange.failure_threshold(),
                            };

                            let mut relay_state = ViewSyncRelayTaskState {
                                phantom: PhantomData,
                                event_stream: self.event_stream.clone(),
                                exchange: self.exchange.clone(),
                                accumulator: either::Left(accumulator),
                            };

                            relay_state = relay_state.handle_event(event2).await.1;

                            let name = format!("View Sync Relay Task");

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
                return;
            }
            // TODO ED Spawn task to start NK20 protocol (if two timeouts have happened)
            // TODO ED Need to add view number to timeout event
            SequencingHotShotEvent::Timeout(view_number) => {
                // TODO ED Clean up this code, pull it out so it isn't repeated from above
                // TODO ED Double check we don't have a task already running?  We shouldn't ever have one
                // TODO ED Only trigger if second timeout
                // TODO ED Send view change event if only first timeout
                let mut replica_state = ViewSyncReplicaTaskState {
                    phantom: PhantomData,
                    current_view: self.current_view,
                    next_view: TYPES::Time::new(*view_number),
                    relay: 0,
                    finalized: false,
                    sent_view_change_event: false,
                    phase: LastSeenViewSyncCeritificate::None,
                    exchange: self.exchange.clone(),
                    api: self.api.clone(),
                    event_stream: self.event_stream.clone(),
                };

                // TODO ED Only if returns that replica state should keep running, if is finalized cert then it should stop
                replica_state = replica_state.handle_event(event2).await.1;

                let name = format!(
                    "View Sync Replica Task: Attempting to enter view {:?} from view {:?}",
                    self.next_view, self.current_view
                );

                // TODO ED Passing in mut state seems to make more sense than a separate function not impled on the state?
                let replica_handle_event = HandleEvent(Arc::new(
                    move |event, mut state: ViewSyncReplicaTaskState<TYPES, I, A>| {
                        async move { state.handle_event(event).await }.boxed()
                    },
                ));

                let filter = FilterEvent::default();
                let _builder = TaskBuilder::<ViewSyncReplicaTaskStateTypes<TYPES, I, A>>::new(name)
                    .register_event_stream(replica_state.event_stream.clone(), filter)
                    .await
                    .register_state(replica_state)
                    .register_event_handler(replica_handle_event);
            }

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
                    let (certificate_internal, last_seen_certificate) = match certificate.clone() {
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

                    // Ignore certificate if it is for an older round
                    if certificate_internal.round < self.next_view {
                        return (None, self);
                    }

                    // If certificate is not valid, return current state
                    if !self
                        .exchange
                        .is_valid_view_sync_cert(certificate, certificate_internal.round.clone())
                    {
                        return (None, self);
                    }

                    // If certificate is for a higher round shutdown this task
                    // since another task should have been started for the higher round
                    // TODO ED Perhaps in the future this should return an error giving more
                    // context
                    if certificate_internal.round > self.next_view {
                        return (Some(HotShotTaskCompleted::ShutDown), self);
                    }

                    // Ignore if the certificate is for an already seen phase
                    // TODO ED Change 'phase' to last_seen_certificate for better clarity
                    if last_seen_certificate <= self.phase {
                        return (None, self);
                    }

                    self.phase = last_seen_certificate;

                    // Send ViewChange event if necessary
                    if self.phase >= LastSeenViewSyncCeritificate::Commit
                        && !self.sent_view_change_event
                    {
                        self.event_stream
                            .publish(SequencingHotShotEvent::ViewChange(ViewNumber::new(
                                *self.next_view,
                            )))
                            .await;
                        self.sent_view_change_event = true;
                    }

                    // The protocol has ended
                    if self.phase == LastSeenViewSyncCeritificate::Finalize {
                        return ((Some(HotShotTaskCompleted::ShutDown)), self);
                    }

                    if certificate_internal.relay > self.relay {
                        self.relay = certificate_internal.relay
                    }

                    // TODO ED Going to assume that nodes must have stake for the view they are voting to enter
                    let maybe_vote_token = self
                        .exchange
                        .membership()
                        .make_vote_token(self.next_view, &self.exchange.private_key());

                    match maybe_vote_token {
                        Ok(Some(vote_token)) => {
                            let message = match self.phase {
                                LastSeenViewSyncCeritificate::None => unimplemented!(),
                                LastSeenViewSyncCeritificate::PreCommit => {
                                    self.exchange.create_commit_message::<I>(
                                        self.next_view,
                                        self.relay,
                                        vote_token.clone(),
                                    )
                                }
                                LastSeenViewSyncCeritificate::Commit => {
                                    self.exchange.create_finalize_message::<I>(
                                        self.next_view,
                                        self.relay,
                                        vote_token.clone(),
                                    )
                                }
                                LastSeenViewSyncCeritificate::Finalize => unimplemented!(),
                            };

                            // TODO ED Perhaps should change the return type of create_x_message functions
                            if let GeneralConsensusMessage::ViewSync(vote) = message {
                                self.event_stream
                                    .publish(SequencingHotShotEvent::ViewSyncMessageSend(vote))
                                    .await;
                            }

                            if self.relay > 0 {
                                let message = match self.phase {
                                    LastSeenViewSyncCeritificate::None => unimplemented!(),
                                    LastSeenViewSyncCeritificate::PreCommit => {
                                        self.exchange.create_precommit_message::<I>(
                                            self.next_view,
                                            0,
                                            vote_token.clone(),
                                        )
                                    }
                                    LastSeenViewSyncCeritificate::Commit => {
                                        self.exchange.create_commit_message::<I>(
                                            self.next_view,
                                            0,
                                            vote_token.clone(),
                                        )
                                    }
                                    LastSeenViewSyncCeritificate::Finalize => unimplemented!(),
                                };
                                if let GeneralConsensusMessage::ViewSync(vote) = message {
                                    self.event_stream
                                        .publish(SequencingHotShotEvent::ViewSyncMessageSend(vote))
                                        .await;
                                }
                            }

                            // TODO ED Add event to shutdown this task
                            async_spawn({
                                let stream = self.event_stream.clone();
                                let phase = self.phase.clone();
                                async move {
                                    async_sleep(Duration::from_millis(10000)).await;
                                    stream
                                        .publish(SequencingHotShotEvent::ViewSyncTimeout(
                                            ViewNumber::new(*self.next_view),
                                            self.relay,
                                            phase,
                                        ))
                                        .await;
                                }
                            });

                            return (None, self);
                        }
                        Ok(None) => todo!(),
                        Err(_) => todo!(),
                    }
                }
                ViewSyncMessageType::Vote(vote) => {
                    // Ignore
                    return (None, self);
                }
            },

            SequencingHotShotEvent::Timeout(view_number) => {
                todo!()
            }

            SequencingHotShotEvent::ViewSyncTimeout(round, relay, last_seen_certificate) => {
                // Shouldn't ever receive a timeout for a relay higher than ours
                // TODO ED Perhaps change types to either TYPES::Time or ViewNumber to avoid conversions
                if TYPES::Time::new(*round) == self.next_view
                    && relay == self.relay
                    && last_seen_certificate == self.phase
                {
                    let maybe_vote_token = self
                        .exchange
                        .membership()
                        .make_vote_token(self.next_view, &self.exchange.private_key());

                    match maybe_vote_token {
                        Ok(Some(vote_token)) => {
                            self.relay = self.relay + 1;
                            let message = match self.phase {
                                LastSeenViewSyncCeritificate::None => {
                                    self.exchange.create_precommit_message::<I>(
                                        self.next_view,
                                        self.relay,
                                        vote_token.clone(),
                                    )
                                }
                                LastSeenViewSyncCeritificate::PreCommit => {
                                    self.exchange.create_commit_message::<I>(
                                        self.next_view,
                                        self.relay,
                                        vote_token.clone(),
                                    )
                                }
                                LastSeenViewSyncCeritificate::Commit => {
                                    self.exchange.create_finalize_message::<I>(
                                        self.next_view,
                                        self.relay,
                                        vote_token.clone(),
                                    )
                                }
                                LastSeenViewSyncCeritificate::Finalize => unimplemented!(),
                            };

                            // TODO ED Perhaps should change the return type of create_x_message functions
                            if let GeneralConsensusMessage::ViewSync(vote) = message {
                                self.event_stream
                                    .publish(SequencingHotShotEvent::ViewSyncMessageSend(vote))
                                    .await;
                            }

                            // TODO ED Add event to shutdown this task
                            async_spawn({
                                let stream = self.event_stream.clone();
                                async move {
                                    // TODO ED Add actual timeout parameter
                                    async_sleep(Duration::from_millis(10000)).await;
                                    stream
                                        .publish(SequencingHotShotEvent::ViewSyncTimeout(
                                            ViewNumber::new(*self.next_view),
                                            self.relay,
                                            last_seen_certificate,
                                        ))
                                        .await;
                                }
                            });
                        }
                        Ok(None) => unimplemented!(),
                        Err(_) => unimplemented!(),
                    }
                }
            }
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
        ViewSyncRelayTaskState<TYPES, I>,
    ) {
        match event {
            SequencingHotShotEvent::ViewSyncMessage(message) => match message {
                ViewSyncMessageType::Certificate(certificate) => {
                    todo!()
                }
                // TODO ED Change name of last seen certificate to just be phase
                ViewSyncMessageType::Vote(vote) => {
                    let (vote_internal, phase) = match vote {
                        ViewSyncVote::PreCommit(vote_internal) => {
                            (vote_internal, LastSeenViewSyncCeritificate::PreCommit)
                        }
                        ViewSyncVote::Commit(vote_internal) => {
                            (vote_internal, LastSeenViewSyncCeritificate::Commit)
                        }
                        ViewSyncVote::Finalize(vote_internal) => {
                            (vote_internal, LastSeenViewSyncCeritificate::Finalize)
                        }
                    };

                    // TODO ED Make another function for this that we can change later
                    // Ignore this vote if we are not the correct relay
                    if !self
                        .exchange
                        .is_leader(vote_internal.round + vote_internal.relay)
                    {
                        return (None, self);
                    }

                    // TODO ED Put actual VoteData here
                    let view_sync_data = 
                        ViewSyncData::<TYPES> {
                            round: vote_internal.round,
                            relay: self.exchange.public_key().to_bytes(),
                        }
                        .commit();

                    let mut accumulator = self.exchange.accumulate_vote(
                        &vote_internal.signature.0,
                        &vote_internal.signature.1,
                        view_sync_data.clone(),
                        vote_internal.vote_data,
                        vote_internal.vote_token.clone(),
                        vote_internal.round,
                        self.accumulator.left().unwrap(),
                    );

                    self.accumulator = match accumulator {
                        Left(new_accumulator) => Either::Left(new_accumulator),
                        Right(certificate) => {
                            // TODO ED Fix enum wrapping so getting data out of View Sync structs isn't so difficult
                            let (certificate_internal, phase) = match certificate.clone() {
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
                            let message = ViewSyncMessageType::Certificate(certificate.clone());
                            self.event_stream
                                .publish(SequencingHotShotEvent::ViewSyncMessageSend(message))
                                .await;
                            Either::Right(certificate)
                        }
                    };

                    if phase == LastSeenViewSyncCeritificate::Finalize {
                        return (Some(HotShotTaskCompleted::ShutDown), self);
                    } else {
                        return (None, self);
                    }
                }
            },
            _ => todo!(),
        }
        return (None, self);
    }
}
