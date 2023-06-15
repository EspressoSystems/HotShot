use crate::event_stream::ChannelStream;
use crate::event_stream::EventStream;
use crate::events::SequencingHotShotEvent;
use crate::task::FilterEvent;
use crate::task::{HandleEvent, HotShotTaskCompleted, TaskErr, TS};
use crate::task_impls::HSTWithEvent;
use crate::task_impls::TaskBuilder;
use async_compatibility_layer::art::{async_sleep, async_spawn};
use async_compatibility_layer::channel::UnboundedReceiver;
use async_lock::{Mutex, RwLock};
#[cfg(feature = "async-std-executor")]
use async_std::task::JoinHandle;
use commit::Committable;
use core::time::Duration;
use either::Either;
use either::Right;
use futures::FutureExt;
use hotshot_consensus::utils::Terminator;
use hotshot_consensus::Consensus;
use hotshot_consensus::SequencingConsensusApi;
use hotshot_types::message::Message;
use hotshot_types::traits::election::ConsensusExchange;
use hotshot_types::traits::election::QuorumExchangeType;
use hotshot_types::traits::node_implementation::{NodeImplementation, SequencingExchangesType};
use hotshot_types::traits::state::ConsensusTime;
use hotshot_types::{
    certificate::{DACertificate, QuorumCertificate},
    data::{QuorumProposal, SequencingLeaf},
    message::{GeneralConsensusMessage, ProcessedSequencingMessage, SequencingMessage},
    traits::{
        consensus_type::sequencing_consensus::SequencingConsensus,
        election::SignedCertificate,
        node_implementation::{CommitteeEx, NodeType, SequencingQuorumEx},
        signature_key::SignatureKey,
    },
    vote::{QuorumVote, VoteAccumulator},
};
use nll::nll_todo::nll_todo;
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

// pub struct TimeoutTaskState {
//     /// shared stream
//     event_stream: MaybePinnedEventStream<HSTT>,
// }

// pub async fn timeout_handle<TYPES: NodeType, I: NodeImplementation<TYPES>>(event: SequencingHotShotEvent<TYPES, I>, state: TimeoutTaskState) -> (std::option::Option<HotShotTaskCompleted>, TimeoutTaskState) {
//     (None, state)
// }

// #[derive(Debug)]
pub struct SequencingConsensusTaskState<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
    >,
    A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + std::fmt::Debug + 'static,
> where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    SequencingQuorumEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,
        Commitment = SequencingLeaf<TYPES>,
    >,
    CommitteeEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Certificate = DACertificate<TYPES>,
        Commitment = TYPES::BlockType,
    >,
{
    /// Reference to consensus. The replica will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES, SequencingLeaf<TYPES>>>>,
    /// Channel for accepting leader proposals and timeouts messages.
    #[allow(clippy::type_complexity)]
    pub proposal_collection_chan:
        Arc<Mutex<UnboundedReceiver<ProcessedSequencingMessage<TYPES, I>>>>,
    /// View number this view is executing in.
    pub cur_view: TYPES::Time,
    /// The High QC.
    pub high_qc: QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,

    /// the quorum exchange
    pub quorum_exchange: Arc<SequencingQuorumEx<TYPES, I>>,

    pub api: A,

    /// the committee exchange
    pub committee_exchange: Arc<CommitteeEx<TYPES, I>>,

    /// needed to typecheck
    pub _pd: PhantomData<I>,

    /// Current Vote collection task, with it's view.
    pub vote_collector: (TYPES::Time, JoinHandle<()>),

    /// timeout task handle
    pub timeout_task: JoinHandle<()>,

    /// Global events stream to publish events
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
}

pub struct VoteCollectionTaskState<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<TYPES, Leaf = SequencingLeaf<TYPES>>,
> where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    SequencingQuorumEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,
        Commitment = SequencingLeaf<TYPES>,
    >,
{
    /// the quorum exchange
    pub quorum_exchange: Arc<SequencingQuorumEx<TYPES, I>>,
    pub accumulator:
        Either<VoteAccumulator<TYPES::VoteTokenType, I::Leaf>, QuorumCertificate<TYPES, I::Leaf>>,
    pub cur_view: TYPES::Time,
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<TYPES, Leaf = SequencingLeaf<TYPES>>,
    > TS for VoteCollectionTaskState<TYPES, I>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    SequencingQuorumEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,
        Commitment = SequencingLeaf<TYPES>,
    >,
{
}

async fn vote_handle<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<TYPES, Leaf = SequencingLeaf<TYPES>>,
>(
    mut state: VoteCollectionTaskState<TYPES, I>,
    event: SequencingHotShotEvent<TYPES, I>,
) -> (
    std::option::Option<HotShotTaskCompleted>,
    VoteCollectionTaskState<TYPES, I>,
)
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    SequencingQuorumEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,
        Commitment = SequencingLeaf<TYPES>,
    >,
{
    match event {
        SequencingHotShotEvent::QuorumVoteRecv(vote, sender) => match vote {
            QuorumVote::Yes(vote) => {
                if vote.signature.0 != <TYPES::SignatureKey as SignatureKey>::to_bytes(&sender) {
                    return (None, state);
                }

                let accumulator = state.accumulator.left().unwrap();
                match state.quorum_exchange.accumulate_vote(
                    &vote.signature.0,
                    &vote.signature.1,
                    vote.leaf_commitment,
                    vote.vote_data,
                    vote.vote_token.clone(),
                    state.cur_view,
                    accumulator,
                ) {
                    Either::Left(acc) => {
                        state.accumulator = Either::Left(acc);
                        return (None, state);
                    }
                    Either::Right(qc) => {
                        state
                            .event_stream
                            .publish(SequencingHotShotEvent::QCFormed(qc.clone()))
                            .await;
                        state.accumulator = Either::Right(qc);
                        return (Some(HotShotTaskCompleted::Success), state);
                    }
                }
            }
            QuorumVote::Timeout(_vote) => {
                return (None, state);
            }
            QuorumVote::No(_) => {
                warn!("The next leader has received an unexpected vote!");
            }
        },
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
        A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + std::fmt::Debug + 'static,
    > SequencingConsensusTaskState<TYPES, I, A>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    SequencingQuorumEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,
        Commitment = SequencingLeaf<TYPES>,
    >,
    CommitteeEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Certificate = DACertificate<TYPES>,
        Commitment = TYPES::BlockType,
    >,
{
    async fn genesis_leaf(&self) -> Option<SequencingLeaf<TYPES>> {
        let consensus = self.consensus.read().await;
        let Some(genesis_view) = consensus.state_map.get(&TYPES::Time::genesis()) else {
            warn!("Couldn't find genesis view in state map.");
            return None;
        };
        let Some(leaf) = genesis_view.get_leaf_commitment() else {
            warn!(
                ?genesis_view,
                "Genesis view points to a view without a leaf"
            );
            return None;
        };
        let Some(leaf) = consensus.saved_leaves.get(&leaf) else {
            warn!("Failed to find genesis leaf.");
            return None;
        };
        Some(leaf.clone())
    }

    pub async fn handle_event(&mut self, event: SequencingHotShotEvent<TYPES, I>) {
        match event {
            SequencingHotShotEvent::QuorumProposalRecv(proposal, sender) => {
                self.timeout_task = async_spawn({
                    // let next_view_timeout = hotshot.inner.config.next_view_timeout;
                    // let next_view_timeout = next_view_timeout;
                    // let hotshot: HotShot<TYPES::ConsensusType, TYPES, I> = hotshot.clone();
                    // TODO(bf): get the real timeout from the config.
                    let stream = self.event_stream.clone();
                    async move {
                        async_sleep(Duration::from_millis(10000)).await;
                        stream.publish(SequencingHotShotEvent::Timeout).await;
                    }
                });
                let consensus = self.consensus.upgradable_read().await;
                let view_leader_key = self.quorum_exchange.get_leader(self.cur_view);
                if view_leader_key != sender {
                    return;
                }

                let vote_token = self.quorum_exchange.make_vote_token(self.cur_view);
                match vote_token {
                    Err(e) => {
                        error!(
                            "Failed to generate vote token for {:?} {:?}",
                            self.cur_view, e
                        );
                    }
                    Ok(None) => {
                        info!(
                            "We were not chosen for consensus committee on {:?}",
                            self.cur_view
                        );
                    }
                    Ok(Some(vote_token)) => {
                        info!(
                            "We were chosen for consensus committee on {:?}",
                            self.cur_view
                        );

                        let message;

                        // Construct the leaf.
                        let justify_qc = proposal.data.justify_qc;
                        let parent = if justify_qc.is_genesis() {
                            self.genesis_leaf().await
                        } else {
                            consensus
                                .saved_leaves
                                .get(&justify_qc.leaf_commitment())
                                .cloned()
                        };
                        let Some(parent) = parent else {
                            warn!("Proposal's parent missing from storage");
                            return;
                        };
                        let parent_commitment = parent.commit();
                        let block_commitment = proposal.data.block_commitment;
                        let leaf: SequencingLeaf<_> = SequencingLeaf {
                            view_number: self.cur_view,
                            height: proposal.data.height,
                            justify_qc: justify_qc.clone(),
                            parent_commitment,
                            deltas: Right(proposal.data.block_commitment),
                            rejected: Vec::new(),
                            timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
                            proposer_id: sender.to_bytes(),
                        };
                        let justify_qc_commitment = justify_qc.commit();
                        let leaf_commitment = leaf.commit();

                        // Validate the `justify_qc`.
                        if !self
                            .quorum_exchange
                            .is_valid_cert(&justify_qc, parent_commitment)
                        {
                            warn!("Invalid justify_qc in proposal!.");
                            message = self.quorum_exchange.create_no_message::<I>(
                                justify_qc_commitment,
                                leaf_commitment,
                                self.cur_view,
                                vote_token,
                            );
                        }
                        // Validate the `height`.
                        else if leaf.height != parent.height + 1 {
                            warn!(
                                "Incorrect height in proposal (expected {}, got {})",
                                parent.height + 1,
                                leaf.height
                            );
                            message = self.quorum_exchange.create_no_message(
                                justify_qc_commitment,
                                leaf_commitment,
                                self.cur_view,
                                vote_token,
                            );
                        }
                        // Validate the DAC.
                        else if !self
                            .committee_exchange
                            .is_valid_cert(&proposal.data.dac, block_commitment)
                        {
                            warn!("Invalid DAC in proposal! Skipping proposal.");
                            message = self.quorum_exchange.create_no_message(
                                justify_qc_commitment,
                                leaf_commitment,
                                self.cur_view,
                                vote_token,
                            );
                        }
                        // Validate the signature.
                        else if !view_leader_key
                            .validate(&proposal.signature, leaf_commitment.as_ref())
                        {
                            warn!(?proposal.signature, "Could not verify proposal.");
                            message = self.quorum_exchange.create_no_message(
                                justify_qc_commitment,
                                leaf_commitment,
                                self.cur_view,
                                vote_token,
                            );
                        }
                        // Create a positive vote if either liveness or safety check
                        // passes.
                        else {
                            // Liveness check.
                            let liveness_check = justify_qc.view_number > consensus.locked_view;

                            // Safety check.
                            // Check if proposal extends from the locked leaf.
                            let outcome = consensus.visit_leaf_ancestors(
                                justify_qc.view_number,
                                Terminator::Inclusive(consensus.locked_view),
                                false,
                                |leaf| {
                                    // if leaf view no == locked view no then we're done, report success by
                                    // returning true
                                    leaf.view_number != consensus.locked_view
                                },
                            );
                            let safety_check = outcome.is_ok();
                            if let Err(e) = outcome {
                                self.api.send_view_error(self.cur_view, Arc::new(e)).await;
                            }

                            // Skip if both saftey and liveness checks fail.
                            if !safety_check && !liveness_check {
                                warn!("Failed safety check and liveness check");
                                message = self.quorum_exchange.create_no_message(
                                    justify_qc_commitment,
                                    leaf_commitment,
                                    self.cur_view,
                                    vote_token,
                                );
                            } else {
                                // Generate a message with yes vote.
                                message = self.quorum_exchange.create_yes_message(
                                    justify_qc_commitment,
                                    leaf_commitment,
                                    self.cur_view,
                                    vote_token,
                                );
                            }
                        }
                        if let GeneralConsensusMessage::Vote(vote) = message {
                            info!("Sending vote to next leader {:?}", vote);
                            self.event_stream
                                .publish(SequencingHotShotEvent::QuorumVoteSend(vote))
                                .await;
                        };
                    }
                }
            }
            SequencingHotShotEvent::QuorumVoteRecv(vote, sender) => {
                match vote {
                    QuorumVote::Yes(vote) => {
                        if vote.signature.0
                            != <TYPES::SignatureKey as SignatureKey>::to_bytes(&sender)
                        {
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
                            success_threshold: self.quorum_exchange.success_threshold(),
                            failure_threshold: self.quorum_exchange.failure_threshold(),
                        };
                        // Todo check if we are the leader
                        let accumulator = self.quorum_exchange.accumulate_vote(
                            &vote.signature.0,
                            &vote.signature.1,
                            vote.leaf_commitment,
                            vote.vote_data,
                            vote.vote_token.clone(),
                            vote.current_view,
                            acc,
                        );
                        if vote.current_view > *collection_view {
                            let state = VoteCollectionTaskState {
                                quorum_exchange: self.quorum_exchange.clone(),
                                accumulator,
                                cur_view: vote.current_view,
                                event_stream: self.event_stream.clone(),
                            };
                            let name = "Quorum Vote Collection";
                            let filter = FilterEvent::default();
                            let _builder =
                                TaskBuilder::<VoteCollectionTypes<TYPES, I>>::new(name.to_string())
                                    .register_event_stream(self.event_stream.clone(), filter)
                                    .await
                                    .register_state(state)
                                    .register_event_handler(handle_event);
                        }
                    }
                    QuorumVote::Timeout(_) | QuorumVote::No(_) => {
                        warn!("The next leader has received an unexpected vote!");
                    }
                }
            }
            SequencingHotShotEvent::ViewSyncMessage => {
                // update the view in state to the one in the message
                nll_todo()
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
        A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + std::fmt::Debug,
    > TS for SequencingConsensusTaskState<TYPES, I, A>
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    SequencingQuorumEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,
        Commitment = SequencingLeaf<TYPES>,
    >,
    CommitteeEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Certificate = DACertificate<TYPES>,
        Commitment = TYPES::BlockType,
    >,
{
}

pub type VoteCollectionTypes<TYPES, I> = HSTWithEvent<
    ConsensusTaskError,
    SequencingHotShotEvent<TYPES, I>,
    ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    VoteCollectionTaskState<TYPES, I>,
>;

pub type ConsensusTaskTypes<TYPES, I, A> = HSTWithEvent<
    ConsensusTaskError,
    SequencingHotShotEvent<TYPES, I>,
    ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    SequencingConsensusTaskState<TYPES, I, A>,
>;

pub async fn sequencing_consensus_handle<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
    >,
    A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + std::fmt::Debug + 'static,
>(
    event: SequencingHotShotEvent<TYPES, I>,
    mut state: SequencingConsensusTaskState<TYPES, I, A>,
) -> (
    std::option::Option<HotShotTaskCompleted>,
    SequencingConsensusTaskState<TYPES, I, A>,
)
where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    SequencingQuorumEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Proposal = QuorumProposal<TYPES, SequencingLeaf<TYPES>>,
        Certificate = QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,
        Commitment = SequencingLeaf<TYPES>,
    >,
    CommitteeEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Certificate = DACertificate<TYPES>,
        Commitment = TYPES::BlockType,
    >,
{
    if let SequencingHotShotEvent::Shutdown = event {
        (Some(HotShotTaskCompleted::ShutDown), state)
    } else {
        state.handle_event(event).await;
        (None, state)
    }
}
