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
use hotshot_task::task::HotShotTaskTypes;
use hotshot_task::task_impls::TaskBuilder;
use hotshot_task::task_launcher::TaskRunner;
use hotshot_task::{
    event_stream::{ChannelStream, EventStream},
    task::{FilterEvent, TaskErr, TS},
    task_impls::HSTWithEvent,
};
use hotshot_types::message::GeneralConsensusMessage::ViewSyncCertificate as ViewSyncCertificateProposal;
use hotshot_types::traits::election::Membership;
use hotshot_types::traits::election::SignedCertificate;
use hotshot_types::traits::election::VoteData;

use hotshot_task::global_registry::GlobalRegistry;
use hotshot_types::certificate::ViewSyncCertificate;
use hotshot_types::data::QuorumProposal;
use hotshot_types::data::SequencingLeaf;
use hotshot_types::data::ViewNumber;
use hotshot_types::message::GeneralConsensusMessage;
use hotshot_types::message::Message;
use hotshot_types::message::Proposal;
use hotshot_types::message::SequencingMessage;
use hotshot_types::traits::consensus_type::sequencing_consensus::SequencingConsensus;
use hotshot_types::traits::election::ConsensusExchange;
use hotshot_types::traits::election::ViewSyncExchangeType;
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

#[derive(PartialEq, PartialOrd, Clone, Debug, Eq, Hash)]
pub enum ViewSyncPhase {
    None,
    PreCommit,
    Commit,
    Finalize,
}

#[derive(Default)]
pub struct ViewSyncTaskInfo {
    task_id: usize,
    event_stream_id: usize,
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
    A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + 'static + std::clone::Clone,
> where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    ViewSyncEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = ViewSyncCertificate<TYPES>,
        Certificate = ViewSyncCertificate<TYPES>,
        Commitment = ViewSyncData<TYPES>,
    >,
{
    pub registry: GlobalRegistry,
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,

    pub current_view: TYPES::Time,
    pub next_view: TYPES::Time,

    pub exchange: Arc<ViewSyncEx<TYPES, I>>,
    pub api: A,

    // pub task_runner: TaskRunner,
    /// How many timeouts we've seen in a row; is reset upon a successful view change
    pub num_timeouts_tracked: u64,

    /// Represents if replica task is running,
    pub replica_task_map: HashMap<TYPES::Time, ViewSyncTaskInfo>,

    /// Represents if relay task is running
    pub relay_task_map: HashMap<TYPES::Time, ViewSyncTaskInfo>,

    pub view_sync_timeout: Duration,
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
        A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + 'static + std::clone::Clone,
    > TS for ViewSyncTaskState<TYPES, I, A>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    ViewSyncEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = ViewSyncCertificate<TYPES>,
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
    A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + 'static,
> where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    ViewSyncEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = ViewSyncCertificate<TYPES>,
        Certificate = ViewSyncCertificate<TYPES>,
        Commitment = ViewSyncData<TYPES>,
    >,
{
    pub view_sync_timeout: Duration,
    pub current_view: TYPES::Time,
    pub next_view: TYPES::Time,
    pub phase: ViewSyncPhase,
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
        A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + 'static,
    > TS for ViewSyncReplicaTaskState<TYPES, I, A>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    ViewSyncEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = ViewSyncCertificate<TYPES>,
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
        A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + 'static + std::clone::Clone,
    > ViewSyncTaskState<TYPES, I, A>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    ViewSyncEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = ViewSyncCertificate<TYPES>,
        Certificate = ViewSyncCertificate<TYPES>,
        Commitment = ViewSyncData<TYPES>,
    >,
{
    pub async fn handle_event(&mut self, event: SequencingHotShotEvent<TYPES, I>) {
        // TODO ED Match on &event
        match event.clone() {
            SequencingHotShotEvent::ViewSyncCertificateRecv(message) => {
                let (certificate_internal, last_seen_certificate) = match message.data {
                    ViewSyncCertificate::PreCommit(certificate_internal) => {
                        (certificate_internal, ViewSyncPhase::PreCommit)
                    }
                    ViewSyncCertificate::Commit(certificate_internal) => {
                        (certificate_internal, ViewSyncPhase::Commit)
                    }
                    ViewSyncCertificate::Finalize(certificate_internal) => {
                        (certificate_internal, ViewSyncPhase::Finalize)
                    }
                };
                error!(
                    "Received view sync cert for phase {:?}",
                    last_seen_certificate
                );

                // This certificate is old, we can throw it away
                // If next view = cert round, then that means we should already have a task running for it
                if self.current_view > certificate_internal.round {
                    error!("Already in a higher view than the view sync message");
                    return;
                }

                if let Some(replica_task) = self.replica_task_map.get(&certificate_internal.round) {
                    // Forward event then return
                    error!("Forwarding message");
                    self.event_stream
                        .direct_message(replica_task.event_stream_id, event)
                        .await;
                    return;
                }

                // We do not have a replica task already running, so start one

                // TODO ED Need to GC old entries in task map once we know we don't need them anymore
                let mut replica_state = ViewSyncReplicaTaskState {
                    current_view: certificate_internal.round,
                    next_view: certificate_internal.round,
                    relay: 0,
                    finalized: false,
                    sent_view_change_event: false,
                    phase: ViewSyncPhase::None,
                    exchange: self.exchange.clone(),
                    api: self.api.clone(),
                    event_stream: self.event_stream.clone(),
                    view_sync_timeout: self.view_sync_timeout,
                };

                let result = replica_state.handle_event(event).await;

                if result.0 == Some(HotShotTaskCompleted::ShutDown) {
                    // The protocol has finished
                    return;
                }

                replica_state = result.1;

                let name = format!(
                    "View Sync Replica Task: Attempting to enter view {:?} from view {:?}",
                    self.next_view, self.current_view
                );

                let replica_handle_event = HandleEvent(Arc::new(
                    move |event, mut state: ViewSyncReplicaTaskState<TYPES, I, A>| {
                        async move { state.handle_event(event).await }.boxed()
                    },
                ));

                let filter = FilterEvent::default();
                let builder = TaskBuilder::<ViewSyncReplicaTaskStateTypes<TYPES, I, A>>::new(name)
                    .register_event_stream(replica_state.event_stream.clone(), filter)
                    .await
                    .register_registry(&mut self.registry.clone())
                    .await
                    .register_state(replica_state)
                    .register_event_handler(replica_handle_event);

                let task_id = builder.get_task_id().unwrap();
                let event_stream_id = builder.get_stream_id().unwrap();

                self.replica_task_map.insert(
                    certificate_internal.round,
                    ViewSyncTaskInfo {
                        task_id,
                        event_stream_id,
                    },
                );

                let _view_sync_replica_task = async_spawn(async move {
                    ViewSyncReplicaTaskStateTypes::build(builder).launch().await
                });
            }

            SequencingHotShotEvent::ViewSyncVoteRecv(vote) => {
                let vote_internal = match vote {
                    ViewSyncVote::PreCommit(vote_internal) => vote_internal,
                    ViewSyncVote::Commit(vote_internal) => vote_internal,
                    ViewSyncVote::Finalize(vote_internal) => vote_internal,
                };

                if let Some(relay_task) = self.relay_task_map.get(&vote_internal.round) {
                    // Forward event then return
                    self.event_stream
                        .direct_message(relay_task.event_stream_id, event)
                        .await;
                    return;
                }

                // We do not have a relay task already running, so start one

                if !self
                    .exchange
                    .is_leader(vote_internal.round + vote_internal.relay)
                {
                    panic!("View sync vote send to wrong leader");
                    return;
                }

                let mut accumulator = VoteAccumulator {
                    total_vote_outcomes: HashMap::new(),
                    yes_vote_outcomes: HashMap::new(),
                    no_vote_outcomes: HashMap::new(),
                    viewsync_precommit_vote_outcomes: HashMap::new(),
                    success_threshold: self.exchange.success_threshold(),
                    failure_threshold: self.exchange.failure_threshold(),
                };

                let mut relay_state = ViewSyncRelayTaskState {
                    event_stream: self.event_stream.clone(),
                    exchange: self.exchange.clone(),
                    accumulator: either::Left(accumulator),
                };

                let result = relay_state.handle_event(event).await;

                if result.0 == Some(HotShotTaskCompleted::ShutDown) {
                    // The protocol has finished
                    return;
                }

                relay_state = result.1;

                let name = format!("View Sync Relay Task for view {:?}", vote_internal.round);

                let relay_handle_event = HandleEvent(Arc::new(
                    move |event, mut state: ViewSyncRelayTaskState<TYPES, I>| {
                        async move { state.handle_event(event).await }.boxed()
                    },
                ));

                let filter = FilterEvent::default();
                let builder = TaskBuilder::<ViewSyncRelayTaskStateTypes<TYPES, I>>::new(name)
                    .register_event_stream(relay_state.event_stream.clone(), filter)
                    .await
                    .register_registry(&mut self.registry.clone())
                    .await
                    .register_state(relay_state)
                    .register_event_handler(relay_handle_event);

                let task_id = builder.get_task_id().unwrap();
                let event_stream_id = builder.get_stream_id().unwrap();

                self.relay_task_map.insert(
                    vote_internal.round,
                    ViewSyncTaskInfo {
                        task_id,
                        event_stream_id,
                    },
                );
                // TODO ED For now we will not await these futures, in the future we can await them only in the case of shutdown
                let _view_sync_relay_task = async_spawn(async move {
                    ViewSyncRelayTaskStateTypes::build(builder).launch().await
                });
            }

            SequencingHotShotEvent::ViewChange(new_view) => {
                // TODO ED Don't call new twice
                if self.current_view < TYPES::Time::new(*new_view) {
                    error!(
                        "Change from view {} to view {} in view sync task",
                        *self.current_view, *new_view
                    );

                    self.current_view = TYPES::Time::new(*new_view);
                    self.next_view = self.current_view; 
                    self.num_timeouts_tracked = 0;

                    // Inject view info into network
                    // self.exchange.network().inject_consensus_info((
                    //     (*new_view),
                    //     self.exchange.is_leader(TYPES::Time::new(*new_view)),
                    //     self.exchange.is_leader(TYPES::Time::new(*new_view) + 1),
                    // ))
                    // .await;
                }
                return;
            }
            SequencingHotShotEvent::Timeout(view_number) => {
                // This is an old timeout and we can ignore it
                if view_number < ViewNumber::new(*self.current_view) {
                    return;
                }
                // TODO ED Combine this code with other replica code since some of it is repeated
                if view_number < ViewNumber::new(*self.current_view) {
                    error!("Got old timeout");
                    return;
                }
                self.num_timeouts_tracked += 1;
                error!("Num timeouts tracked is {}", self.num_timeouts_tracked);

                // TODO ED Make this a configurable variable
                if self.num_timeouts_tracked >= 2 {
                    // panic!("Starting view sync!");
                    // Spawn replica task
                    let mut replica_state = ViewSyncReplicaTaskState {
                        current_view: self.current_view,
                        next_view: TYPES::Time::new(*view_number + 1),
                        relay: 0,
                        finalized: false,
                        sent_view_change_event: false,
                        phase: ViewSyncPhase::None,
                        exchange: self.exchange.clone(),
                        api: self.api.clone(),
                        event_stream: self.event_stream.clone(),
                        view_sync_timeout: self.view_sync_timeout,
                    };

                    // TODO ED Make all these view numbers into a single variable to avoid errors
                    let result = replica_state
                        .handle_event(SequencingHotShotEvent::ViewSyncTrigger(view_number + 1))
                        .await;

                    if result.0 == Some(HotShotTaskCompleted::ShutDown) {
                        // The protocol has finished
                        return;
                    }

                    replica_state = result.1;

                    let name = format!(
                        "View Sync Replica Task: Attempting to enter view {:?} from view {:?}",
                        self.next_view, self.current_view
                    );

                    let replica_handle_event = HandleEvent(Arc::new(
                        move |event, mut state: ViewSyncReplicaTaskState<TYPES, I, A>| {
                            async move { state.handle_event(event).await }.boxed()
                        },
                    ));

                    // TODO ED Change from default filter
                    let filter = FilterEvent::default();
                    let builder =
                        TaskBuilder::<ViewSyncReplicaTaskStateTypes<TYPES, I, A>>::new(name)
                            .register_event_stream(replica_state.event_stream.clone(), filter)
                            .await
                            .register_registry(&mut self.registry.clone())
                            .await
                            .register_state(replica_state)
                            .register_event_handler(replica_handle_event);

                    let task_id = builder.get_task_id().unwrap();
                    let event_stream_id = builder.get_stream_id().unwrap();

                    self.replica_task_map.insert(
                        TYPES::Time::new(*view_number + 1),
                        ViewSyncTaskInfo {
                            task_id,
                            event_stream_id,
                        },
                    );

                    // TODO ED For now we will not await these futures, in the future we can await them only in the case of shutdown
                    let _view_sync_replica_task = async_spawn(async move {
                        ViewSyncReplicaTaskStateTypes::build(builder).launch().await
                    });
                } else {
                    // If this is the first timeout we've seen advance to the next view
                    self.current_view += 1;
                    self.event_stream
                        .publish(SequencingHotShotEvent::ViewChange(ViewNumber::new(
                            *self.current_view,
                        )))
                        .await;
                }
            }

            _ => return,
        }
    }

    /// Filter view sync related events.
    pub fn filter(event: &SequencingHotShotEvent<TYPES, I>) -> bool {
        match event {
            SequencingHotShotEvent::ViewSyncCertificateRecv(_)
            | SequencingHotShotEvent::ViewSyncCertificateSend(_, _)
            | SequencingHotShotEvent::ViewSyncVoteRecv(_)
            | SequencingHotShotEvent::ViewSyncVoteSend(_)
            | SequencingHotShotEvent::Shutdown
            | SequencingHotShotEvent::Timeout(_)
            | SequencingHotShotEvent::ViewSyncTimeout(_, _, _)
            | SequencingHotShotEvent::ViewChange(_) => true,
            _ => false,
        }
    }
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
        A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + 'static,
    > ViewSyncReplicaTaskState<TYPES, I, A>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    ViewSyncEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = ViewSyncCertificate<TYPES>,
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
            SequencingHotShotEvent::ViewSyncCertificateRecv(message) => {
                let (certificate_internal, last_seen_certificate) = match message.data.clone() {
                    ViewSyncCertificate::PreCommit(certificate_internal) => {
                        (certificate_internal, ViewSyncPhase::PreCommit)
                    }
                    ViewSyncCertificate::Commit(certificate_internal) => {
                        (certificate_internal, ViewSyncPhase::Commit)
                    }
                    ViewSyncCertificate::Finalize(certificate_internal) => {
                        (certificate_internal, ViewSyncPhase::Finalize)
                    }
                };

                error!("received cert in handle_event for replica");

                // Ignore certificate if it is for an older round
                if certificate_internal.round < self.next_view {
                    error!("We're already in a higher round");

                    return (None, self);
                }

                let relay_key = self
                    .exchange
                    .get_leader(certificate_internal.round + certificate_internal.relay);

                if !relay_key.validate(&message.signature, &message.data.commit().as_ref()) {
                    error!("Key does not validate for certificate sender");
                    return (None, self);
                }

                // If certificate is not valid, return current state
                if !self
                    .exchange
                    .is_valid_view_sync_cert(message.data, certificate_internal.round.clone())
                {
                    error!("Not valid view sync cert!");

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
                if last_seen_certificate <= self.phase {
                    return (None, self);
                }

                self.phase = last_seen_certificate;

                // Send ViewChange event if necessary
                if self.phase >= ViewSyncPhase::Commit && !self.sent_view_change_event {
                    error!("Updating view from view sync to view {}", *self.next_view);
                    self.event_stream
                        .publish(SequencingHotShotEvent::ViewChange(ViewNumber::new(
                            *self.next_view,
                        )))
                        .await;
                    self.sent_view_change_event = true;
                }

                // The protocol has ended
                if self.phase == ViewSyncPhase::Finalize {
                    return ((Some(HotShotTaskCompleted::ShutDown)), self);
                }

                if certificate_internal.relay > self.relay {
                    self.relay = certificate_internal.relay
                }

                // TODO ED Assuming that nodes must have stake for the view they are voting to enter
                let maybe_vote_token = self
                    .exchange
                    .membership()
                    .make_vote_token(self.next_view, &self.exchange.private_key());

                match maybe_vote_token {
                    Ok(Some(vote_token)) => {
                        let message = match self.phase {
                            ViewSyncPhase::None => unimplemented!(),
                            ViewSyncPhase::PreCommit => self.exchange.create_commit_message::<I>(
                                self.next_view,
                                self.relay,
                                vote_token.clone(),
                            ),
                            ViewSyncPhase::Commit => self.exchange.create_finalize_message::<I>(
                                self.next_view,
                                self.relay,
                                vote_token.clone(),
                            ),
                            // Should never hit this
                            ViewSyncPhase::Finalize => unimplemented!(),
                        };

                        if let GeneralConsensusMessage::ViewSyncVote(vote) = message {
                            self.event_stream
                                .publish(SequencingHotShotEvent::ViewSyncVoteSend(vote))
                                .await;
                        }

                        // Send to the first relay after sending to k_th relay
                        if self.relay > 0 {
                            let message = match self.phase {
                                ViewSyncPhase::None => unimplemented!(),
                                ViewSyncPhase::PreCommit => {
                                    self.exchange.create_precommit_message::<I>(
                                        self.next_view,
                                        0,
                                        vote_token.clone(),
                                    )
                                }
                                ViewSyncPhase::Commit => self.exchange.create_commit_message::<I>(
                                    self.next_view,
                                    0,
                                    vote_token.clone(),
                                ),
                                ViewSyncPhase::Finalize => unimplemented!(),
                            };
                            if let GeneralConsensusMessage::ViewSyncVote(vote) = message {
                                self.event_stream
                                    .publish(SequencingHotShotEvent::ViewSyncVoteSend(vote))
                                    .await;
                            }
                        }

                        // TODO ED Add event to shutdown this task if a view is completed
                        async_spawn({
                            let stream = self.event_stream.clone();
                            let phase = self.phase.clone();
                            async move {
                                async_sleep(self.view_sync_timeout).await;
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
                    Ok(None) => return (None, self),
                    Err(_) => return (None, self),
                }
            }
            SequencingHotShotEvent::ViewSyncVoteRecv(vote) => {
                // Ignore
                return (None, self);
            }

            // The main ViewSync task should handle this
            SequencingHotShotEvent::Timeout(view_number) => return (None, self),

            SequencingHotShotEvent::ViewSyncTrigger(view_number) => {
                // Trigger protocol by sending the first precommit vote, assumes view number passed in is the next view we want to enter
                let maybe_vote_token = self
                    .exchange
                    .membership()
                    .make_vote_token(self.next_view, &self.exchange.private_key());

                match maybe_vote_token {
                    Ok(Some(vote_token)) => {
                        self.relay = self.relay;
                        let message = self.exchange.create_precommit_message::<I>(
                            self.next_view,
                            self.relay,
                            vote_token.clone(),
                        );

                        if let GeneralConsensusMessage::ViewSyncVote(vote) = message {
                            error!(
                                "Sending precommit vote to start protocol for next view = {}",
                                *vote.round()
                            );

                            self.event_stream
                                .publish(SequencingHotShotEvent::ViewSyncVoteSend(vote))
                                .await;
                        }

                        // TODO ED Add event to shutdown this task
                        async_spawn({
                            let stream = self.event_stream.clone();
                            async move {
                                async_sleep(self.view_sync_timeout).await;
                                stream
                                    .publish(SequencingHotShotEvent::ViewSyncTimeout(
                                        ViewNumber::new(*self.next_view),
                                        self.relay,
                                        ViewSyncPhase::None,
                                    ))
                                    .await;
                            }
                        });
                        return (None, self);
                    }
                    _ => {
                        error!("Problem generating vote token");
                        return (None, self);
                    }
                }
            }

            SequencingHotShotEvent::ViewSyncTimeout(round, relay, last_seen_certificate) => {
                // Shouldn't ever receive a timeout for a relay higher than ours
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
                                ViewSyncPhase::None => self.exchange.create_precommit_message::<I>(
                                    self.next_view,
                                    self.relay,
                                    vote_token.clone(),
                                ),
                                ViewSyncPhase::PreCommit => {
                                    self.exchange.create_commit_message::<I>(
                                        self.next_view,
                                        self.relay,
                                        vote_token.clone(),
                                    )
                                }
                                ViewSyncPhase::Commit => {
                                    self.exchange.create_finalize_message::<I>(
                                        self.next_view,
                                        self.relay,
                                        vote_token.clone(),
                                    )
                                }
                                ViewSyncPhase::Finalize => unimplemented!(),
                            };

                            if let GeneralConsensusMessage::ViewSyncVote(vote) = message {
                                self.event_stream
                                    .publish(SequencingHotShotEvent::ViewSyncVoteSend(vote))
                                    .await;
                            }

                            // TODO ED Add event to shutdown this task
                            async_spawn({
                                let stream = self.event_stream.clone();
                                async move {
                                    async_sleep(self.view_sync_timeout).await;
                                    stream
                                        .publish(SequencingHotShotEvent::ViewSyncTimeout(
                                            ViewNumber::new(*self.next_view),
                                            self.relay,
                                            last_seen_certificate,
                                        ))
                                        .await;
                                }
                            });
                            return (None, self);
                        }
                        Ok(None) => return (None, self),
                        Err(_) => return (None, self),
                    }
                }
            }
            _ => return (None, self),
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
        Proposal = ViewSyncCertificate<TYPES>,
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
            SequencingHotShotEvent::ViewSyncCertificateRecv(message) => return (None, self),
            SequencingHotShotEvent::ViewSyncVoteRecv(vote) => {
                if self.accumulator.is_right() {
                    return (Some(HotShotTaskCompleted::ShutDown), self);
                }

                let (vote_internal, phase) = match vote {
                    ViewSyncVote::PreCommit(vote_internal) => {
                        (vote_internal, ViewSyncPhase::PreCommit)
                    }
                    ViewSyncVote::Commit(vote_internal) => (vote_internal, ViewSyncPhase::Commit),
                    ViewSyncVote::Finalize(vote_internal) => {
                        (vote_internal, ViewSyncPhase::Finalize)
                    }
                };

                error!(
                    "Recved vote for next view {}, and relay {}, and phase {:?}",
                    *vote_internal.round, vote_internal.relay, phase
                );

                // Ignore this vote if we are not the correct relay
                if !self
                    .exchange
                    .is_leader(vote_internal.round + vote_internal.relay)
                {
                    error!("We are not the correct relay");
                    return (None, self);
                }

                let view_sync_data = ViewSyncData::<TYPES> {
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
                    Some(vote_internal.relay),
                );

                self.accumulator = match accumulator {
                    Left(new_accumulator) => Either::Left(new_accumulator),
                    Right(certificate) => {
                        let signature =
                            self.exchange.sign_certificate_proposal(certificate.clone());
                        let message = Proposal {
                            data: certificate.clone(),
                            signature,
                        };
                        self.event_stream
                            .publish(SequencingHotShotEvent::ViewSyncCertificateSend(
                                message,
                                self.exchange.public_key().clone(),
                            ))
                            .await;

                        // Reset accumulator for new certificate
                        either::Left(VoteAccumulator {
                            total_vote_outcomes: HashMap::new(),
                            yes_vote_outcomes: HashMap::new(),
                            no_vote_outcomes: HashMap::new(),
                            viewsync_precommit_vote_outcomes: HashMap::new(),

                            success_threshold: self.exchange.success_threshold(),
                            failure_threshold: self.exchange.failure_threshold(),
                        })
                    }
                };

                if phase == ViewSyncPhase::Finalize {
                    return (Some(HotShotTaskCompleted::ShutDown), self);
                } else {
                    return (None, self);
                }
            }
            _ => return (None, self),
        }
        return (None, self);
    }
}
