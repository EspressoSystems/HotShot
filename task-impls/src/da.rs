use crate::events::SequencingHotShotEvent;
use async_compatibility_layer::art::{async_sleep, async_spawn};
use async_compatibility_layer::channel::UnboundedReceiver;
use async_lock::{Mutex, RwLock};
#[cfg(feature = "async-std-executor")]
use async_std::task::JoinHandle;
use commit::Committable;
use core::time::Duration;
use either::Either;
use either::{Left, Right};
use futures::FutureExt;
use hotshot_consensus::Consensus;
use hotshot_consensus::SequencingConsensusApi;
use hotshot_task::event_stream::ChannelStream;
use hotshot_task::event_stream::EventStream;
use hotshot_task::task::FilterEvent;
use hotshot_task::task::{HandleEvent, HotShotTaskCompleted, TaskErr, TS};
use hotshot_task::task_impls::HSTWithEvent;
use hotshot_task::task_impls::TaskBuilder;
use hotshot_types::message::{CommitteeConsensusMessage, Message};
use hotshot_types::traits::election::{CommitteeExchangeType, ConsensusExchange};
use hotshot_types::traits::node_implementation::{NodeImplementation, SequencingExchangesType};
use hotshot_types::{
    certificate::{DACertificate, QuorumCertificate},
    data::{ProposalType, SequencingLeaf},
    message::{ProcessedSequencingMessage, SequencingMessage},
    traits::{
        consensus_type::sequencing_consensus::SequencingConsensus,
        node_implementation::{CommitteeEx, NodeType},
        signature_key::SignatureKey,
    },
    vote::VoteAccumulator,
};
use snafu::Snafu;
use std::collections::HashMap;
use std::marker::PhantomData;
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
    /// View number this view is executing in.
    pub cur_view: TYPES::Time,
    /// The `high_qc` per spec
    pub high_qc: QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,

    /// the committee exchange
    pub committee_exchange: Arc<CommitteeEx<TYPES, I>>,

    /// Current Vote collection task, with it's view.
    pub vote_collector: (TYPES::Time, JoinHandle<()>),

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
    pub cur_view: TYPES::Time,
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
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
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
        SequencingHotShotEvent::DAVoteRecv(vote, sender) => {
            if vote.signature.0 != <TYPES::SignatureKey as SignatureKey>::to_bytes(&sender) {
                return (None, state);
            }

            let accumulator = state.accumulator.left().unwrap();
            match state.committee_exchange.accumulate_vote(
                &vote.signature.0,
                &vote.signature.1,
                vote.block_commitment,
                vote.vote_data,
                vote.vote_token.clone(),
                state.cur_view,
                accumulator,
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
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
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
    pub async fn handle_event(&mut self, event: SequencingHotShotEvent<TYPES, I>) {
        match event {
            SequencingHotShotEvent::DAProposalRecv(proposal, sender) => {
                let view = proposal.data.get_view_number();
                if view < self.cur_view {
                    return;
                }
                let block_commitment = proposal.data.deltas.commit();
                let view_leader_key = self.committee_exchange.get_leader(view);
                if view_leader_key != sender {
                    return;
                }
                if !view_leader_key.validate(&proposal.signature, block_commitment.as_ref()) {
                    warn!(?proposal.signature, "Could not verify proposal.");
                    return;
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
                        let message = self.committee_exchange.create_da_message::<I>(
                            self.high_qc.commit(),
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
            SequencingHotShotEvent::DAVoteRecv(vote, sender) => {
                if vote.signature.0 != <TYPES::SignatureKey as SignatureKey>::to_bytes(&sender) {
                    return;
                }
                let handle_event = HandleEvent(Arc::new(move |event, state| {
                    async move { vote_handle(state, event).await }.boxed()
                }));
                let (collection_view, _collection_task) = &self.vote_collector;
                let acc = VoteAccumulator {
                    total_vote_outcomes: HashMap::new(),
                    yes_vote_outcomes: HashMap::new(),
                    no_vote_outcomes: HashMap::new(),
                    success_threshold: self.committee_exchange.success_threshold(),
                    failure_threshold: self.committee_exchange.failure_threshold(),
                };
                // Todo check if we are the leader
                let accumulator = self.committee_exchange.accumulate_vote(
                    &vote.signature.0,
                    &vote.signature.1,
                    vote.block_commitment,
                    vote.vote_data,
                    vote.vote_token.clone(),
                    vote.current_view,
                    acc,
                );
                if vote.current_view > *collection_view {
                    let state = DAVoteCollectionTaskState {
                        committee_exchange: self.committee_exchange.clone(),
                        accumulator,
                        cur_view: vote.current_view,
                        event_stream: self.event_stream.clone(),
                    };
                    let name = "DA Vote Collection";
                    let filter = FilterEvent::default();
                    // TODO (run_view refactor) `TaskBuilder` is created but the task isn't added to the task runner.
                    let _builder =
                        TaskBuilder::<DAVoteCollectionTypes<TYPES, I>>::new(name.to_string())
                            .register_event_stream(self.event_stream.clone(), filter)
                            .await
                            .register_state(state)
                            .register_event_handler(handle_event);
                }
            }
            _ => {}
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
