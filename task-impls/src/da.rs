use crate::events::SequencingHotShotEvent;
use async_compatibility_layer::art::async_spawn;
#[cfg(feature = "async-std-executor")]
use async_std::task::JoinHandle;
use commit::Committable;
use either::Either;
use either::{Left, Right};
use futures::FutureExt;
use hotshot_task::event_stream::ChannelStream;
use hotshot_task::event_stream::EventStream;
use hotshot_task::global_registry::GlobalRegistry;
use hotshot_task::task::FilterEvent;
use hotshot_task::task::{HandleEvent, HotShotTaskCompleted, HotShotTaskTypes, TaskErr, TS};
use hotshot_task::task_impls::HSTWithEvent;
use hotshot_task::task_impls::TaskBuilder;
use hotshot_types::message::{CommitteeConsensusMessage, Message};
use hotshot_types::traits::election::{CommitteeExchangeType, ConsensusExchange};
use hotshot_types::traits::node_implementation::{NodeImplementation, SequencingExchangesType};
use hotshot_types::{
    certificate::DACertificate,
    data::{ProposalType, SequencingLeaf, ViewNumber},
    message::SequencingMessage,
    traits::{
        consensus_type::sequencing_consensus::SequencingConsensus,
        node_implementation::{CommitteeEx, NodeType},
        signature_key::SignatureKey,
        state::ConsensusTime,
    },
    vote::VoteAccumulator,
};
use snafu::Snafu;
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(feature = "tokio-executor")]
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

#[derive(Snafu, Debug)]
pub struct ConsensusTaskError {}
impl TaskErr for ConsensusTaskError {}

pub struct DATaskState<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
    >,
> where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    CommitteeEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Certificate = DACertificate<TYPES>,
        Commitment = TYPES::BlockType,
    >,
{
    pub registry: GlobalRegistry,

    /// View number this view is executing in.
    pub cur_view: ViewNumber,

    /// the committee exchange
    pub committee_exchange: Arc<CommitteeEx<TYPES, I>>,

    /// Current Vote collection task, with it's view and ID.
    pub vote_collector: (ViewNumber, usize, JoinHandle<HotShotTaskCompleted>),

    /// Global events stream to publish events
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
}

pub struct DAVoteCollectionTaskState<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<TYPES, Leaf = SequencingLeaf<TYPES>>,
> where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    CommitteeEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Certificate = DACertificate<TYPES>,
        Commitment = TYPES::BlockType,
    >,
{
    /// the committee exchange
    pub committee_exchange: Arc<CommitteeEx<TYPES, I>>,
    pub accumulator:
        Either<VoteAccumulator<TYPES::VoteTokenType, TYPES::BlockType>, DACertificate<TYPES>>,
    pub cur_view: ViewNumber,
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, Leaf = SequencingLeaf<TYPES>>,
    > TS for DAVoteCollectionTaskState<TYPES, I>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    CommitteeEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Certificate = DACertificate<TYPES>,
        Commitment = TYPES::BlockType,
    >,
{
}

async fn vote_handle<
    TYPES: NodeType<ConsensusType = SequencingConsensus, Time = ViewNumber>,
    I: NodeImplementation<TYPES, Leaf = SequencingLeaf<TYPES>>,
>(
    mut state: DAVoteCollectionTaskState<TYPES, I>,
    event: SequencingHotShotEvent<TYPES, I>,
) -> (
    std::option::Option<HotShotTaskCompleted>,
    DAVoteCollectionTaskState<TYPES, I>,
)
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    CommitteeEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Certificate = DACertificate<TYPES>,
        Commitment = TYPES::BlockType,
    >,
{
    match event {
        SequencingHotShotEvent::DAVoteRecv(vote) => {
            let accumulator = state.accumulator.left().unwrap();
            match state.committee_exchange.accumulate_vote(
                &vote.signature.0,
                &vote.signature.1,
                vote.block_commitment,
                vote.vote_data,
                vote.vote_token.clone(),
                state.cur_view,
                accumulator,
                None,
            ) {
                Left(acc) => {
                    state.accumulator = Either::Left(acc);
                    return (None, state);
                }
                Right(dac) => {
                    state
                        .event_stream
                        .publish(SequencingHotShotEvent::DACSend(dac.clone()))
                        .await;
                    state.accumulator = Right(dac);
                    return (None, state);
                }
            }
        }
        SequencingHotShotEvent::Shutdown => return (Some(HotShotTaskCompleted::ShutDown), state),
        _ => {}
    }
    (None, state)
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus, Time = ViewNumber>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
    > DATaskState<TYPES, I>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    CommitteeEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Certificate = DACertificate<TYPES>,
        Commitment = TYPES::BlockType,
    >,
{
    pub async fn handle_event(
        &mut self,
        event: SequencingHotShotEvent<TYPES, I>,
    ) -> Option<HotShotTaskCompleted> {
        match event {
            SequencingHotShotEvent::DAProposalRecv(proposal, sender) => {
                let view = proposal.data.get_view_number();
                if view < self.cur_view {
                    return None;
                }
                let block_commitment = proposal.data.deltas.commit();
                let view_leader_key = self.committee_exchange.get_leader(view);
                if view_leader_key != sender {
                    return None;
                }
                if !view_leader_key.validate(&proposal.signature, block_commitment.as_ref()) {
                    warn!(?proposal.signature, "Could not verify proposal.");
                    return None;
                }

                let vote_token = self.committee_exchange.make_vote_token(view);
                match vote_token {
                    Err(e) => {
                        error!("Failed to generate vote token for {:?} {:?}", view, e);
                    }
                    Ok(None) => {
                        info!("We were not chosen for DA committee on {:?}", view);
                    }
                    Ok(Some(vote_token)) => {
                        info!("We were chosen for DA committee on {:?}", view);

                        // Generate and send vote
                        let message = self.committee_exchange.create_da_message(
                            block_commitment,
                            view,
                            vote_token,
                        );

                        self.cur_view = view;

                        if let CommitteeConsensusMessage::DAVote(vote) = message {
                            info!("Sending vote to the DA leader {:?}", vote);
                            self.event_stream
                                .publish(SequencingHotShotEvent::DAVoteSend(vote))
                                .await;
                        }
                    }
                }
            }
            SequencingHotShotEvent::DAVoteRecv(vote) => {
                // Check if we are the leader and the vote is from the sender.
                let view = vote.current_view;
                if &self.committee_exchange.get_leader(view) != self.committee_exchange.public_key()
                {
                    return None;
                }

                let handle_event = HandleEvent(Arc::new(move |event, state| {
                    async move { vote_handle(state, event).await }.boxed()
                }));
                let (collection_view, collection_id, _collection_task) = &self.vote_collector;
                if view > *collection_view {
                    self.registry.shutdown_task(*collection_id).await;
                }
                let acc = VoteAccumulator {
                    total_vote_outcomes: HashMap::new(),
                    yes_vote_outcomes: HashMap::new(),
                    no_vote_outcomes: HashMap::new(),
                    success_threshold: self.committee_exchange.success_threshold(),
                    failure_threshold: self.committee_exchange.failure_threshold(),
                    viewsync_precommit_vote_outcomes: HashMap::new(),
                };
                let accumulator = self.committee_exchange.accumulate_vote(
                    &vote.signature.0,
                    &vote.signature.1,
                    vote.block_commitment,
                    vote.vote_data,
                    vote.vote_token.clone(),
                    view,
                    acc,
                    None,
                );
                if view > *collection_view {
                    let state = DAVoteCollectionTaskState {
                        committee_exchange: self.committee_exchange.clone(),
                        accumulator,
                        cur_view: view,
                        event_stream: self.event_stream.clone(),
                    };
                    let name = "DA Vote Collection";
                    let filter = FilterEvent::default();
                    let builder =
                        TaskBuilder::<DAVoteCollectionTypes<TYPES, I>>::new(name.to_string())
                            .register_event_stream(self.event_stream.clone(), filter)
                            .await
                            .register_registry(&mut self.registry.clone())
                            .await
                            .register_state(state)
                            .register_event_handler(handle_event);
                    let id = builder.get_task_id().unwrap();
                    let task =
                        async_spawn(
                            async move { DAVoteCollectionTypes::build(builder).launch().await },
                        );
                    self.vote_collector = (view, id, task);
                }
            }
            SequencingHotShotEvent::ViewChange(view) => {
                self.cur_view = view;
                return None;
            }
            SequencingHotShotEvent::Shutdown => {
                let (_, collection_id, _) = &self.vote_collector;
                self.registry.shutdown_task(*collection_id);
                return Some(HotShotTaskCompleted::ShutDown);
            }
            _ => {}
        }
        None
    }

    /// Filter the DA event.
    pub fn filter(event: &SequencingHotShotEvent<TYPES, I>) -> bool {
        match event {
            SequencingHotShotEvent::DAProposalRecv(_, _)
            | SequencingHotShotEvent::DAVoteRecv(_)
            | SequencingHotShotEvent::Shutdown
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
    > TS for DATaskState<TYPES, I>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    CommitteeEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Certificate = DACertificate<TYPES>,
        Commitment = TYPES::BlockType,
    >,
{
}

pub type DAVoteCollectionTypes<TYPES, I> = HSTWithEvent<
    ConsensusTaskError,
    SequencingHotShotEvent<TYPES, I>,
    ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    DAVoteCollectionTaskState<TYPES, I>,
>;

pub type DATaskTypes<TYPES, I> = HSTWithEvent<
    ConsensusTaskError,
    SequencingHotShotEvent<TYPES, I>,
    ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    DATaskState<TYPES, I>,
>;
