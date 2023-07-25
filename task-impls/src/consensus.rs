use crate::events::SequencingHotShotEvent;
use async_compatibility_layer::art::{async_sleep, async_spawn};
use async_lock::RwLock;
use async_lock::RwLockUpgradableReadGuard;
#[cfg(feature = "async-std-executor")]
use async_std::task::JoinHandle;
use bincode::config::Options;
use commit::Committable;
use core::time::Duration;
use either::Either;
use either::Left;
use either::Right;
use futures::FutureExt;
use hotshot_consensus::utils::Terminator;
use hotshot_consensus::utils::ViewInner;
use hotshot_consensus::Consensus;
use hotshot_consensus::SequencingConsensusApi;
use hotshot_consensus::View;
use hotshot_task::event_stream::ChannelStream;
use hotshot_task::event_stream::EventStream;
use hotshot_task::global_registry::GlobalRegistry;
use hotshot_task::task::FilterEvent;
use hotshot_task::task::HotShotTaskTypes;
use hotshot_task::task::{HandleEvent, HotShotTaskCompleted, TaskErr, TS};
use hotshot_task::task_impls::HSTWithEvent;
use hotshot_task::task_impls::TaskBuilder;
use hotshot_types::data::LeafType;
use hotshot_types::data::ProposalType;
use hotshot_types::data::ViewNumber;
use hotshot_types::event::{Event, EventType};
use hotshot_types::message::Message;
use hotshot_types::message::Proposal;
use hotshot_types::traits::election::ConsensusExchange;
use hotshot_types::traits::election::QuorumExchangeType;
use hotshot_types::traits::network::CommunicationChannel;
use hotshot_types::traits::network::ConsensusIntentEvent;
use hotshot_types::traits::node_implementation::{NodeImplementation, SequencingExchangesType};
use hotshot_types::traits::state::ConsensusTime;
use hotshot_types::traits::Block;
use hotshot_types::vote::VoteType;
use hotshot_types::{
    certificate::{DACertificate, QuorumCertificate},
    data::{QuorumProposal, SequencingLeaf},
    message::{GeneralConsensusMessage, SequencingMessage},
    traits::{
        consensus_type::sequencing_consensus::SequencingConsensus,
        election::SignedCertificate,
        node_implementation::{CommitteeEx, NodeType, SequencingQuorumEx},
        signature_key::SignatureKey,
    },
    vote::{QuorumVote, VoteAccumulator},
};
use hotshot_utils::bincode::bincode_opts;
use snafu::Snafu;
use std::collections::HashMap;
use std::collections::HashSet;
use std::marker::PhantomData;
use std::sync::Arc;
#[cfg(feature = "tokio-executor")]
use tokio::task::JoinHandle;
use tracing::{error, info, instrument, warn};

#[derive(Snafu, Debug)]
pub struct ConsensusTaskError {}
impl TaskErr for ConsensusTaskError {}

// #[derive(Debug)]
pub struct SequencingConsensusTaskState<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
    >,
    A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + 'static,
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
    pub registry: GlobalRegistry,
    /// Reference to consensus. The replica will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES, SequencingLeaf<TYPES>>>>,
    /// View number this view is executing in.
    pub cur_view: ViewNumber,

    /// Current block submitted to DA
    pub block: TYPES::BlockType,

    /// the quorum exchange
    pub quorum_exchange: Arc<SequencingQuorumEx<TYPES, I>>,

    pub api: A,

    /// the committee exchange
    pub committee_exchange: Arc<CommitteeEx<TYPES, I>>,

    /// needed to typecheck
    pub _pd: PhantomData<I>,

    /// Current Vote collection task, with it's view.
    pub vote_collector: Option<(ViewNumber, usize, usize)>,

    /// Have we already sent a proposal for a particular view
    /// since proposal can be sent either on QCFormed event or ViewChange event
    // pub proposal_sent: HashMap<TYPES::Time, bool>,

    /// timeout task handle
    pub timeout_task: JoinHandle<()>,

    /// Global events stream to publish events
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,

    /// Event stream to publish events to the application layer
    pub output_event_stream: ChannelStream<Event<TYPES, I::Leaf>>,

    /// All the DA certs we've received for current and future views.
    pub certs: HashMap<ViewNumber, DACertificate<TYPES>>,

    /// The most recent proposal we have, will correspond to the current view if Some()
    /// Will be none if the view advanced through timeout/view_sync
    pub current_proposal: Option<QuorumProposal<TYPES, I::Leaf>>,

    // ED Should replace this with config information since we need it anyway
    pub id: u64,

    pub qc: Option<QuorumCertificate<TYPES, I::Leaf>>,
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
    #[allow(clippy::type_complexity)]
    pub accumulator:
        Either<VoteAccumulator<TYPES::VoteTokenType, I::Leaf>, QuorumCertificate<TYPES, I::Leaf>>,
    pub cur_view: TYPES::Time,
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    pub id: u64,
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

#[instrument(skip_all, fields(id = state.id, view = *state.cur_view), name = "Quorum Vote Collection Task", level = "error")]

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
    // TODO ED Emit a view change event upon new proposal?
    match event {
        SequencingHotShotEvent::QuorumVoteRecv(vote) => match vote {
            QuorumVote::Yes(vote) => {
                // warn!("Received quroum vote in collector: {:?}", vote);

                // For the case where we receive votes after we've made a certificate
                if state.accumulator.is_right() {
                    // warn!("Already made qc");
                    return (None, state);
                }

                if vote.current_view != state.cur_view {
                    error!(
                        "Vote view does not match! vote view is {} current view is {}",
                        *vote.current_view, *state.cur_view
                    );
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
                    None,
                ) {
                    Either::Left(acc) => {
                        state.accumulator = Either::Left(acc);
                        return (None, state);
                    }
                    Either::Right(qc) => {
                        warn!("QCFormed! {:?}", qc.view_number);
                        state
                            .event_stream
                            .publish(SequencingHotShotEvent::QCFormed(qc.clone()))
                            .await;
                        state.accumulator = Either::Right(qc.clone());

                        // No longer need to poll for votes
                        let _ = state
                            .quorum_exchange
                            .network()
                            .inject_consensus_info(ConsensusIntentEvent::CancelPollForVotes(
                                *qc.view_number,
                            ))
                            .await;

                        return (Some(HotShotTaskCompleted::ShutDown), state);
                    }
                }
            }
            QuorumVote::Timeout(_vote) => {
                error!("The next leader has received an unexpected vote!");
                return (None, state);
            }
            QuorumVote::No(_) => {
                panic!("The next leader has received an unexpected vote!");
            }
        },
        SequencingHotShotEvent::Shutdown => {
            // warn!("Shutting down vote handle");
            return (Some(HotShotTaskCompleted::ShutDown), state);
        }
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
        A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + 'static,
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
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Consensus genesis leaf", level = "error")]

    async fn genesis_leaf(&self) -> Option<SequencingLeaf<TYPES>> {
        let consensus = self.consensus.read().await;

        let Some(genesis_view) = consensus.state_map.get(&TYPES::Time::genesis()) else {
            error!("Couldn't find genesis view in state map.");
            return None;
        };
        let Some(leaf) = genesis_view.get_leaf_commitment() else {
            error!(
                ?genesis_view,
                "Genesis view points to a view without a leaf"
            );
            return None;
        };
        let Some(leaf) = consensus.saved_leaves.get(&leaf) else {
            error!("Failed to find genesis leaf.");
            return None;
        };
        Some(leaf.clone())
    }

    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Consensus vote if able", level = "error")]

    async fn vote_if_able(&self) -> bool {
        // warn!("In vote if able");

        if let Some(proposal) = &self.current_proposal {
            // ED Need to account for the genesis DA cert
            // // warn!("in vote if able for proposal view {:?}", proposal.view_number);

            if proposal.justify_qc.is_genesis() && proposal.view_number == ViewNumber::new(1) {
                // warn!("Proposal is genesis!");

                let view = TYPES::Time::new(*proposal.view_number);
                let vote_token = self.quorum_exchange.make_vote_token(view);

                match vote_token {
                    Err(e) => {
                        error!("Failed to generate vote token for {:?} {:?}", view, e);
                    }
                    Ok(None) => {
                        info!("We were not chosen for consensus committee on {:?}", view);
                    }
                    Ok(Some(vote_token)) => {
                        let justify_qc = proposal.justify_qc.clone();
                        let parent = if justify_qc.is_genesis() {
                            self.genesis_leaf().await
                        } else {
                            self.consensus
                                .read()
                                .await
                                .saved_leaves
                                .get(&justify_qc.leaf_commitment())
                                .cloned()
                        };

                        // Justify qc's leaf commitment is not the same as the parent's leaf commitment, but it should be (in this case)
                        let Some(parent) = parent else {
                            error!("Proposal's parent missing from storage with commitment: {:?}", justify_qc.leaf_commitment());
                            return false;
                        };
                        let parent_commitment = parent.commit();

                        let leaf: SequencingLeaf<_> = SequencingLeaf {
                            view_number: view,
                            height: proposal.height,
                            justify_qc: proposal.justify_qc.clone(),
                            parent_commitment,
                            deltas: Right(proposal.block_commitment),
                            rejected: Vec::new(),
                            timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
                            proposer_id: self.quorum_exchange.get_leader(view).to_bytes(),
                        };

                        let message: GeneralConsensusMessage<TYPES, I> =
                            self.quorum_exchange.create_yes_message(
                                proposal.justify_qc.commit(),
                                leaf.commit(),
                                view,
                                vote_token,
                            );

                        if let GeneralConsensusMessage::Vote(vote) = message {
                            warn!(
                                "Sending vote to next quorum leader {:?}",
                                vote.current_view()
                            );
                            self.event_stream
                                .publish(SequencingHotShotEvent::QuorumVoteSend(vote))
                                .await;
                            return true;
                        }
                    }
                }
            }

            // Only vote if you have the DA cert
            // ED Need to update the view number this is stored under?
            if let Some(cert) = self.certs.get(&(proposal.get_view_number())) {
                let view = cert.view_number;
                let vote_token = self.quorum_exchange.make_vote_token(view);
                // TODO: do some of this logic without the vote token check, only do that when voting.
                match vote_token {
                    Err(e) => {
                        error!("Failed to generate vote token for {:?} {:?}", view, e);
                    }
                    Ok(None) => {
                        info!("We were not chosen for consensus committee on {:?}", view);
                    }
                    Ok(Some(vote_token)) => {
                        let justify_qc = proposal.justify_qc.clone();
                        let parent = if justify_qc.is_genesis() {
                            self.genesis_leaf().await
                        } else {
                            self.consensus
                                .read()
                                .await
                                .saved_leaves
                                .get(&justify_qc.leaf_commitment())
                                .cloned()
                        };

                        // Justify qc's leaf commitment is not the same as the parent's leaf commitment, but it should be (in this case)
                        let Some(parent) = parent else {
                            error!("Proposal's parent missing from storage with commitment: {:?}", justify_qc.leaf_commitment());
                            return false;
                        };
                        let parent_commitment = parent.commit();

                        let leaf: SequencingLeaf<_> = SequencingLeaf {
                            view_number: view,
                            height: proposal.height,
                            justify_qc: proposal.justify_qc.clone(),
                            parent_commitment,
                            deltas: Right(proposal.block_commitment),
                            rejected: Vec::new(),
                            timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
                            proposer_id: self.quorum_exchange.get_leader(view).to_bytes(),
                        };
                        let message: GeneralConsensusMessage<TYPES, I>=
                        // Validate the DAC.
                        if !self
                            .committee_exchange
                            .is_valid_cert(cert, proposal.block_commitment)
                        {
                            error!("Invalid DAC in proposal! Skipping proposal. {:?} cur view is: {:?}", cert.view_number, self.cur_view );
                            return false;
                            // message = self.quorum_exchange.create_no_message(
                            //     proposal.justify_qc.commit(),
                            //     proposal.justify_qc.leaf_commitment,
                            //     cert.view_number,
                            //     vote_token,
                            // );
                        } else {
                            self.quorum_exchange.create_yes_message(
                                proposal.justify_qc.commit(),
                                leaf.commit(),
                                cert.view_number,
                                vote_token)
                        };

                        // TODO ED Only publish event in vote if able
                        if let GeneralConsensusMessage::Vote(vote) = message {
                            warn!(
                                "Sending vote to next quorum leader {:?}",
                                vote.current_view()
                            );
                            self.event_stream
                                .publish(SequencingHotShotEvent::QuorumVoteSend(vote))
                                .await;
                            return true;
                        }
                    }
                }
            }
            warn!(
                "Couldn't find DAC cert in certs, meaning we haven't received it yet for view {:?}",
                *proposal.get_view_number(),
            );
            return false;
        }
        warn!(
            "Could not vote because we don't have a proposal yet for view {}",
            *self.cur_view
        );
        false
    }

    /// Must only update the view and GC if the view actually changes
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Consensus update view", level = "error")]

    async fn update_view(&mut self, new_view: ViewNumber) -> bool {
        if *self.cur_view < *new_view {
            warn!(
                "Updating view from {} to {} in consensus task",
                *self.cur_view, *new_view
            );

            // Remove old certs, we won't vote on past views
            // TODO ED Put back in once we fix other errors
            // for view in *self.cur_view..*new_view - 1 {
            //     let v = ViewNumber::new(view);
            //     self.certs.remove(&v);
            // }
            self.cur_view = new_view;
            self.current_proposal = None;

            // Start polling for proposals for the new view
            // error!("Polling for quorum proposal for view {}", *new_view);

            let _ = self
                .quorum_exchange
                .network()
                .inject_consensus_info(ConsensusIntentEvent::PollForProposal(*self.cur_view))
                .await;

            let _ = self
                .quorum_exchange
                .network()
                .inject_consensus_info(ConsensusIntentEvent::PollForDAC(*self.cur_view))
                .await;

            if self.quorum_exchange.is_leader(self.cur_view + 1) {
                error!("Polling for quorum votes for view {}", *self.cur_view);
                let _ = self
                    .quorum_exchange
                    .network()
                    .inject_consensus_info(ConsensusIntentEvent::PollForVotes(*self.cur_view))
                    .await;
            }

            self.event_stream
                .publish(SequencingHotShotEvent::ViewChange(new_view))
                .await;

            // Spawn a timeout task if we did actually update view
            self.timeout_task = async_spawn({
                // let next_view_timeout = hotshot.inner.config.next_view_timeout;
                // let next_view_timeout = next_view_timeout;
                // let hotshot: HotShot<TYPES::ConsensusType, TYPES, I> = hotshot.clone();
                // TODO(bf): get the real timeout from the config.
                let stream = self.event_stream.clone();
                let view_number = self.cur_view;
                async move {
                    // ED: Changing to 1 second to test timeout logic
                    async_sleep(Duration::from_millis(30000)).await;
                    stream
                        .publish(SequencingHotShotEvent::Timeout(ViewNumber::new(
                            *view_number,
                        )))
                        .await;
                }
            });

            return true;
        }
        false
    }

    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Consensus replica task", level = "error")]

    pub async fn handle_event(&mut self, event: SequencingHotShotEvent<TYPES, I>) {
        match event {
            SequencingHotShotEvent::QuorumProposalRecv(proposal, sender) => {
                error!(
                    "Receved Quorum Propsoal for view {}",
                    *proposal.data.view_number
                );

                let view = proposal.data.get_view_number();
                if view < self.cur_view {
                    error!("view to high");
                    return;
                }

                let view_leader_key = self.quorum_exchange.get_leader(view);
                if view_leader_key != sender {
                    error!("Leader key does not match key in proposal");
                    return;
                }

                // self.update_view(view).await;
                // warn!("After {:?}  sender: {:?}", *view, sender);

                // warn!("Current view: {:?}", self.cur_view);

                self.current_proposal = Some(proposal.data.clone());

                let vote_token = self.quorum_exchange.make_vote_token(view);
                // TODO: do some of this logic without the vote token check, only do that when voting.
                match vote_token {
                    Err(e) => {
                        error!("Failed to generate vote token for {:?} {:?}", view, e);
                    }
                    Ok(None) => {
                        info!("We were not chosen for consensus committee on {:?}", view);
                    }
                    Ok(Some(vote_token)) => {
                        info!("We were chosen for consensus committee on {:?}", view);
                        let consensus = self.consensus.upgradable_read().await;
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

                        // Justify qc's leaf commitment is not the same as the parent's leaf commitment, but it should be (in this case)
                        let Some(parent) = parent else {
                            error!("Proposal's parent missing from storage with commitment: {:?}", justify_qc.leaf_commitment());
                            return;
                        };
                        let parent_commitment = parent.commit();
                        let _block_commitment = proposal.data.block_commitment;
                        let leaf: SequencingLeaf<_> = SequencingLeaf {
                            view_number: view,
                            height: proposal.data.height,
                            justify_qc: justify_qc.clone(),
                            parent_commitment,
                            deltas: Right(proposal.data.block_commitment),
                            rejected: Vec::new(),
                            timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
                            proposer_id: sender.to_bytes(),
                        };
                        // warn!("Leaf replica is voting on! {:?}", leaf.commit());
                        let justify_qc_commitment = justify_qc.commit();
                        let leaf_commitment = leaf.commit();

                        // Validate the `justify_qc`.
                        if !self
                            .quorum_exchange
                            .is_valid_cert(&justify_qc, parent_commitment)
                        {
                            error!("Invalid justify_qc in proposal!. parent commitment is {:?} justify qc is {:?}", parent_commitment, justify_qc.clone());

                            message = self.quorum_exchange.create_no_message::<I>(
                                justify_qc_commitment,
                                leaf_commitment,
                                view,
                                vote_token,
                            );
                        }
                        // Validate the `height`.
                        else if leaf.height != parent.height + 1 {
                            error!(
                                "Incorrect height in proposal (expected {}, got {})",
                                parent.height + 1,
                                leaf.height
                            );
                            message = self.quorum_exchange.create_no_message(
                                justify_qc_commitment,
                                leaf_commitment,
                                view,
                                vote_token,
                            );
                        }
                        // Validate the signature.
                        else if !view_leader_key
                            .validate(&proposal.signature, leaf_commitment.as_ref())
                        {
                            error!(?proposal.signature, "Could not verify proposal.");
                            message = self.quorum_exchange.create_no_message(
                                justify_qc_commitment,
                                leaf_commitment,
                                view,
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
                                self.api.send_view_error(view, Arc::new(e)).await;
                            }

                            // Skip if both saftey and liveness checks fail.
                            if !safety_check && !liveness_check {
                                error!("Failed safety check and liveness check");
                                message = self.quorum_exchange.create_no_message(
                                    justify_qc_commitment,
                                    leaf_commitment,
                                    view,
                                    vote_token,
                                );
                            } else {
                                // Generate a message with yes vote.

                                message = self.quorum_exchange.create_yes_message(
                                    justify_qc_commitment,
                                    leaf_commitment,
                                    view,
                                    vote_token,
                                );
                            }
                        }

                        let high_qc = leaf.justify_qc.clone();
                        let mut new_anchor_view = consensus.last_decided_view;
                        let mut new_locked_view = consensus.locked_view;
                        let mut last_view_number_visited = view;
                        let mut new_commit_reached: bool = false;
                        let mut new_decide_reached = false;
                        let mut new_decide_qc = None;
                        let mut leaf_views = Vec::new();
                        let mut included_txns = HashSet::new();
                        let old_anchor_view = consensus.last_decided_view;
                        let parent_view = leaf.justify_qc.view_number;
                        let mut current_chain_length = 0usize;
                        if parent_view + 1 == view {
                            current_chain_length += 1;
                            if let Err(e) = consensus.visit_leaf_ancestors(
                            parent_view,
                            Terminator::Exclusive(old_anchor_view),
                            true,
                            |leaf| {
                                if !new_decide_reached {
                                    if last_view_number_visited == leaf.view_number + 1 {
                                        last_view_number_visited = leaf.view_number;
                                        current_chain_length += 1;
                                        if current_chain_length == 2 {
                                            new_locked_view = leaf.view_number;
                                            new_commit_reached = true;
                                            // The next leaf in the chain, if there is one, is decided, so this
                                            // leaf's justify_qc would become the QC for the decided chain.
                                            new_decide_qc = Some(leaf.justify_qc.clone());
                                        } else if current_chain_length == 3 {
                                            new_anchor_view = leaf.view_number;
                                            new_decide_reached = true;
                                        }
                                    } else {
                                        // nothing more to do here... we don't have a new chain extension
                                        return false;
                                    }
                                }
                                // starting from the first iteration with a three chain, e.g. right after the else if case nested in the if case above
                                if new_decide_reached {
                                    let mut leaf = leaf.clone();

                                    // If the full block is available for this leaf, include it in the leaf
                                    // chain that we send to the client.
                                    if let Some(block) =
                                        consensus.saved_blocks.get(leaf.get_deltas_commitment())
                                    {
                                        if let Err(err) = leaf.fill_deltas(block.clone()) {
                                            error!("unable to fill leaf {} with block {}, block will not be available: {}",
                                                leaf.commit(), block.commit(), err);
                                        }
                                    }

                                    leaf_views.push(leaf.clone());
                                    match &leaf.deltas {
                                        Left(block) => {
                                            let txns = block.contained_transactions();
                                            for txn in txns {
                                                included_txns.insert(txn);
                                            }
                                        }
                                        Right(_) => {}
                                }
                            }
                                true
                            },
                        ) {
                            error!("publishing view error");
                            self.output_event_stream.publish(Event {
                                view_number: view,
                                event: EventType::Error { error: e.into() },
                            }).await;
                        }
                        }

                        let included_txns_set: HashSet<_> = if new_decide_reached {
                            included_txns
                        } else {
                            HashSet::new()
                        };

                        // promote lock here to add proposal to statemap
                        let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;
                        if high_qc.view_number > consensus.high_qc.view_number {
                            consensus.high_qc = high_qc;
                        }
                        consensus.state_map.insert(
                            view,
                            View {
                                view_inner: ViewInner::Leaf {
                                    leaf: leaf.commit(),
                                },
                            },
                        );
                        consensus.saved_leaves.insert(leaf.commit(), leaf.clone());
                        if new_commit_reached {
                            consensus.locked_view = new_locked_view;
                        }
                        #[allow(clippy::cast_precision_loss)]
                        if new_decide_reached {
                            consensus
                                .transactions
                                .modify(|txns| {
                                    *txns = txns
                                        .drain()
                                        .filter(|(txn_hash, txn)| {
                                            if included_txns_set.contains(txn_hash) {
                                                false
                                            } else {
                                                true
                                            }
                                        })
                                        .collect();
                                })
                                .await;

                            warn!("about to publish decide");
                            let decide_sent = self.output_event_stream.publish(Event {
                                view_number: consensus.last_decided_view,
                                event: EventType::Decide {
                                    leaf_chain: Arc::new(leaf_views),
                                    qc: Arc::new(new_decide_qc.unwrap()),
                                    block_size: Some(included_txns_set.len().try_into().unwrap()),
                                },
                            });
                            let old_anchor_view = consensus.last_decided_view;
                            consensus
                                .collect_garbage(old_anchor_view, new_anchor_view)
                                .await;
                            consensus.last_decided_view = new_anchor_view;
                            consensus.invalid_qc = 0;

                            // We're only storing the last QC. We could store more but we're realistically only going to retrieve the last one.
                            if let Err(e) = self.api.store_leaf(old_anchor_view, leaf).await {
                                error!("Could not insert new anchor into the storage API: {:?}", e);
                            }

                            // warn!("Inserting leaf into storage {:?}", leaf.commit());
                            if *view % 10 == 0 && self.quorum_exchange.is_leader(view) {
                                error!("Sending Decide for view {:?}", consensus.last_decided_view);
                                error!("Decided txns {:?}", included_txns_set)
                            }
                            decide_sent.await;
                        }

                        let new_view = self.current_proposal.clone().unwrap().view_number + 1;
                        // In future we can use the mempool model where we fetch the proposal if we don't have it, instead of having to wait for it here
                        let should_propose = self.quorum_exchange.is_leader(new_view)
                            && consensus.high_qc.view_number
                                == self.current_proposal.clone().unwrap().view_number;
                        // todo get rid of this clone
                        let qc = consensus.high_qc.clone();

                        drop(consensus);
                        if should_propose {
                            error!(
                                "Attempting to publish proposal after voting; now in view: {}",
                                *new_view
                            );
                            self.publish_proposal_if_able(qc).await;
                        }
                        if !self.vote_if_able().await {
                            return;
                        }

                        // ED Only do this GC if we are able to vote
                        for v in *self.cur_view..*view + 1 {
                            let time = TYPES::Time::new(v);
                            self.certs.remove(&time);
                        }
                        // warn!("Voting for view {}", *self.cur_view);
                        // self.current_proposal = None;

                        // Update current view and publish a view change event so other tasks also update
                        self.update_view(new_view).await;

                        if let GeneralConsensusMessage::Vote(vote) = message {
                            info!("Sending vote to next leader {:?}", vote);
                            // warn!("Vote is {:?}", vote.current_view());

                            // self.event_stream
                            //     .publish(SequencingHotShotEvent::QuorumVoteSend(vote))
                            //     .await;
                        };
                    }
                }
            }
            SequencingHotShotEvent::QuorumVoteRecv(vote) => {
                warn!("Received quroum vote: {:?}", vote.current_view());

                if !self.quorum_exchange.is_leader(vote.current_view() + 1) {
                    error!(
                        "We are not the leader for view {} are we the leader for view + 1? {}",
                        *vote.current_view() + 1,
                        self.quorum_exchange.is_leader(vote.current_view() + 2)
                    );
                    return;
                }

                match vote {
                    QuorumVote::Yes(vote) => {
                        // warn!(
                        //     "Recved quorum vote outside of vote handle for view {:?}",
                        //     vote.current_view
                        // );
                        let handle_event = HandleEvent(Arc::new(move |event, state| {
                            async move { vote_handle(state, event).await }.boxed()
                        }));
                        let collection_view = if let Some((collection_view, collection_task, _)) =
                            &self.vote_collector
                        {
                            if vote.current_view > *collection_view {
                                // ED I think we'd want to let that task timeout to avoid a griefing vector
                                self.registry.shutdown_task(*collection_task).await;
                            }
                            *collection_view
                        } else {
                            ViewNumber::new(0)
                        };

                        let acc = VoteAccumulator {
                            total_vote_outcomes: HashMap::new(),
                            yes_vote_outcomes: HashMap::new(),
                            no_vote_outcomes: HashMap::new(),
                            viewsync_precommit_vote_outcomes: HashMap::new(),

                            success_threshold: self.quorum_exchange.success_threshold(),
                            failure_threshold: self.quorum_exchange.failure_threshold(),
                        };
                        // Todo check if we are the leader
                        let accumulator = self.quorum_exchange.accumulate_vote(
                            &vote.signature.0,
                            &vote.signature.1,
                            vote.leaf_commitment,
                            vote.vote_data.clone(),
                            vote.vote_token.clone(),
                            vote.current_view,
                            acc,
                            None,
                        );

                        if vote.current_view > collection_view {
                            let state = VoteCollectionTaskState {
                                quorum_exchange: self.quorum_exchange.clone(),
                                accumulator,
                                cur_view: vote.current_view,
                                event_stream: self.event_stream.clone(),
                                id: self.id,
                            };
                            let name = "Quorum Vote Collection";
                            let filter = FilterEvent(Arc::new(|event| {
                                matches!(event, SequencingHotShotEvent::QuorumVoteRecv(_))
                            }));

                            let builder =
                                TaskBuilder::<VoteCollectionTypes<TYPES, I>>::new(name.to_string())
                                    .register_event_stream(self.event_stream.clone(), filter)
                                    .await
                                    .register_registry(&mut self.registry.clone())
                                    .await
                                    .register_state(state)
                                    .register_event_handler(handle_event);
                            let id = builder.get_task_id().unwrap();
                            let stream_id = builder.get_stream_id().unwrap();

                            self.vote_collector = Some((vote.current_view, id, stream_id));

                            let _task = async_spawn(async move {
                                VoteCollectionTypes::build(builder).launch().await;
                            });
                            warn!("Starting vote handle for view {:?}", vote.current_view);
                        } else if let Some((_, _, stream_id)) = self.vote_collector {
                            self.event_stream
                                .direct_message(
                                    stream_id,
                                    SequencingHotShotEvent::QuorumVoteRecv(QuorumVote::Yes(vote)),
                                )
                                .await;
                        }
                    }
                    QuorumVote::Timeout(_) | QuorumVote::No(_) => {
                        panic!("The next leader has received an unexpected vote!");
                    }
                }
            }
            SequencingHotShotEvent::QCFormed(qc) => {
                warn!("QC Formed event happened!");

                let mut consensus = self.consensus.write().await;
                consensus.high_qc = qc.clone();

                drop(consensus);

                // warn!("QC leaf commitment is {:?}", qc.leaf_commitment());
                // self.event_stream
                //     .publish(SequencingHotShotEvent::ViewChange(qc.view_number() + 1))
                //     .await;

                // View may have already been updated by replica if they voted for this QC
                // TODO ED We should separate leader state from replica state, they shouldn't share the same view
                // Leader task should only run for a specific view, and never update its current view, but instead spawn another task
                // let _res = self.update_view(qc.view_number + 1).await;

                // Start polling for votes for the next view
                // if _res {
                // if self.quorum_exchange.is_leader(qc.view_number + 2) {
                //     self.quorum_exchange
                //         .network()
                //         .inject_consensus_info(
                //             (ConsensusIntentEvent::PollForVotes(*qc.view_number + 1)),
                //         )
                //         .await;
                // }
                // }

                // warn!("Handle qc formed event!");
                // TODO ED Why isn't cur view correct here?
                // // So we don't create a QC on the first view unless we are the leader
                error!(
                    "Attempting to publish proposal after forming a QC for view {}",
                    *qc.view_number
                );

                if self.publish_proposal_if_able(qc.clone()).await {
                    self.update_view(qc.view_number + 1).await;
                }
            }
            SequencingHotShotEvent::DACRecv(cert) => {
                warn!("DAC Recved for view ! {}", *cert.view_number);

                let view = cert.view_number;
                self.certs.insert(view, cert);

                // TODO Make sure we aren't voting for an arbitrarily old round for no reason
                if self.vote_if_able().await {
                    self.update_view(view + 1).await;
                    // TODO ED We should make the event publishing part of update_view function
                    // self.event_stream
                    //     .publish(SequencingHotShotEvent::ViewChange(self.cur_view))
                    //     .await;
                }
            }

            SequencingHotShotEvent::ViewChange(new_view) => {
                warn!("View Change event for view {}", *new_view);

                let old_view_number = self.cur_view;

                // update the view in state to the one in the message
                // ED Update_view return a bool whether it actually updated
                // Publish a view change event to the application
                self.output_event_stream
                    .publish(Event {
                        view_number: old_view_number,
                        event: EventType::ViewFinished {
                            view_number: old_view_number,
                        },
                    })
                    .await;
                if !self.update_view(new_view).await {
                    return;
                }

                error!("View Change event for view {}", *new_view);

                // ED Need to update the view here?  What does otherwise?
                // self.update_view(qc.view_number + 1).await;
                // So we don't create a QC on the first view unless we are the leader
                if !self.quorum_exchange.is_leader(self.cur_view) {
                    return;
                }

                // update our high qc to the qc we just formed
                // self.high_qc = qc;
                let consensus = self.consensus.read().await;
                let parent_view_number = &consensus.high_qc.view_number();
                let mut reached_decided = false;

                let Some(parent_view) = consensus.state_map.get(parent_view_number) else {
                    error!("Couldn't find high QC parent in state map.");
                    return;
                };
                let Some(leaf) = parent_view.get_leaf_commitment() else {
                    error!(
                        ?parent_view_number,
                        ?parent_view,
                        "Parent of high QC points to a view without a proposal"
                    );
                    return;
                };
                let Some(leaf) = consensus.saved_leaves.get(&leaf) else {
                    error!("Failed to find high QC parent.");
                    return;
                };
                if leaf.view_number == consensus.last_decided_view {
                    reached_decided = true;
                }
                let parent_leaf = leaf.clone();

                let original_parent_hash = parent_leaf.commit();

                let mut next_parent_hash = original_parent_hash;

                if !reached_decided {
                    warn!("not reached decide fro view {:?}", self.cur_view);
                    while let Some(next_parent_leaf) = consensus.saved_leaves.get(&next_parent_hash)
                    {
                        if next_parent_leaf.view_number <= consensus.last_decided_view {
                            break;
                        }
                        next_parent_hash = next_parent_leaf.parent_commitment;
                    }
                    // TODO do some sort of sanity check on the view number that it matches decided
                    warn!("updated saved leaves");
                }

                let block_commitment = self.block.commit();
                let leaf = SequencingLeaf {
                    view_number: self.cur_view,
                    height: parent_leaf.height + 1,
                    justify_qc: consensus.high_qc.clone(),
                    parent_commitment: parent_leaf.commit(),
                    // Use the block commitment rather than the block, so that the replica can construct
                    // the same leaf with the commitment.
                    deltas: Right(block_commitment),
                    rejected: vec![],
                    timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
                    proposer_id: self.api.public_key().to_bytes(),
                };
                let signature = self
                    .quorum_exchange
                    .sign_validating_or_commitment_proposal::<I>(&leaf.commit());
                // TODO: DA cert is sent as part of the proposal here, we should split this out so we don't have to wait for it.
                let proposal = QuorumProposal {
                    block_commitment,
                    view_number: leaf.view_number,
                    height: leaf.height,
                    justify_qc: consensus.high_qc.clone(),
                    proposer_id: leaf.proposer_id,
                    dac: None,
                };

                let message = Proposal {
                    data: proposal,
                    signature,
                };
                // warn!("Sending proposal for view {:?} \n {:?}", self.cur_view, message.clone());
                warn!("Sending proposal for view {:?}", message.data.clone());

                self.event_stream
                    .publish(SequencingHotShotEvent::QuorumProposalSend(
                        message,
                        self.quorum_exchange.public_key().clone(),
                    ))
                    .await;
            }
            SequencingHotShotEvent::Timeout(view) => {
                // The view sync module will handle updating views in the case of timeout
                // TODO ED In the future send a timeout vote
                let _ = self
                    .quorum_exchange
                    .network()
                    .inject_consensus_info(ConsensusIntentEvent::CancelPollForVotes(*view))
                    .await;
                error!(
                    "We received a timeout event in the consensus task for view {}!",
                    *view
                )
            }
            SequencingHotShotEvent::SendDABlockData(block) => {
                // ED TODO Should make sure this is actually the most recent block
                // warn!("Updating self . block!");
                self.block = block;
            }
            _ => {}
        }
    }

    pub async fn publish_proposal_if_able(&self, qc: QuorumCertificate<TYPES, I::Leaf>) -> bool {
        if !self.quorum_exchange.is_leader(qc.view_number + 1) {
            error!("Somehow we formed a QC but are not the leader for the next view");
            return false;
        }

        // update our high qc to the qc we just formed
        // self.high_qc = qc;
        let consensus = self.consensus.read().await;
        let parent_view_number = &consensus.high_qc.view_number();
        // error!("Parent view number is {:?}", parent_view_number);
        let mut reached_decided = false;

        let Some(parent_view) = consensus.state_map.get(parent_view_number) else {
            // This should have been added by the replica? 
            error!("Couldn't find parent view in state map, waiting for replica to see proposal\n parent view number: {}", **parent_view_number);
            return false;
        };
        // Leaf hash in view inner does not match high qc hash - Why?
        let Some(leaf_commitment) = parent_view.get_leaf_commitment() else {
            error!(
                ?parent_view_number,
                ?parent_view,
                "Parent of high QC points to a view without a proposal"
            );
            return false;
        };
        if leaf_commitment != consensus.high_qc.leaf_commitment() {
            // error!(
            //     "They don't equal: {:?}   {:?}",
            //     leaf_commitment,
            //     self.high_qc.leaf_commitment()
            // );
        }
        let Some(leaf) = consensus.saved_leaves.get(&leaf_commitment) else {
            error!("Failed to find high QC of parent.");
            return false;
        };
        if leaf.view_number == consensus.last_decided_view {
            reached_decided = true;
        }

        let parent_leaf = leaf.clone();

        let original_parent_hash = parent_leaf.commit();

        let mut next_parent_hash = original_parent_hash;

        // Walk back until we find a decide
        if !reached_decided {
            warn!("not reached decide fro view {:?}", self.cur_view);
            while let Some(next_parent_leaf) = consensus.saved_leaves.get(&next_parent_hash) {
                if next_parent_leaf.view_number <= consensus.last_decided_view {
                    break;
                }
                next_parent_hash = next_parent_leaf.parent_commitment;
            }
            warn!("updated saved leaves");
            // TODO do some sort of sanity check on the view number that it matches decided
        }

        let block_commitment = self.block.commit();
        if block_commitment == TYPES::BlockType::new().commit() {
            error!("Block is generic block! {:?}", self.cur_view);
        }
        // warn!(
        //     "leaf commitment of new qc: {:?}",
        //     self.high_qc.leaf_commitment()
        // );
        let leaf = SequencingLeaf {
            view_number: *parent_view_number + 1,
            height: parent_leaf.height + 1,
            justify_qc: consensus.high_qc.clone(),
            parent_commitment: parent_leaf.commit(),
            // Use the block commitment rather than the block, so that the replica can construct
            // the same leaf with the commitment.
            deltas: Right(block_commitment),
            rejected: vec![],
            timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            proposer_id: self.api.public_key().to_bytes(),
        };
        // warn!("Leaf sent in proposal! {:?}", parent_leaf.commit());

        let signature = self
            .quorum_exchange
            .sign_validating_or_commitment_proposal::<I>(&leaf.commit());
        // TODO: DA cert is sent as part of the proposal here, we should split this out so we don't have to wait for it.
        let proposal = QuorumProposal {
            block_commitment,
            view_number: leaf.view_number,
            height: leaf.height,
            justify_qc: consensus.high_qc.clone(),
            proposer_id: leaf.proposer_id,
            dac: None,
        };

        let message = Proposal {
            data: proposal,
            signature,
        };
        error!("Sending proposal for view {:?} \n {:?}", self.cur_view, "");

        // warn!("Sending proposal for view {:?}", message.data.clone());

        self.event_stream
            .publish(SequencingHotShotEvent::QuorumProposalSend(
                message,
                self.quorum_exchange.public_key().clone(),
            ))
            .await;
        true
    }
}

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
        A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I>,
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
    TYPES: NodeType<ConsensusType = SequencingConsensus, Time = ViewNumber>,
    I: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
    >,
    A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + 'static,
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

pub fn consensus_event_filter<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    event: &SequencingHotShotEvent<TYPES, I>,
) -> bool {
    matches!(
        event,
        SequencingHotShotEvent::QuorumProposalRecv(_, _)
            | SequencingHotShotEvent::QuorumVoteRecv(_)
            | SequencingHotShotEvent::QCFormed(_)
            | SequencingHotShotEvent::DACRecv(_)
            | SequencingHotShotEvent::ViewChange(_)
            | SequencingHotShotEvent::SendDABlockData(_)
            | SequencingHotShotEvent::Timeout(_)
    )
}
