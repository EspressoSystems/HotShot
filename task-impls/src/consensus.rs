use crate::events::SequencingHotShotEvent;
use async_compatibility_layer::art::{async_sleep, async_spawn};

use async_lock::RwLock;
#[cfg(feature = "async-std-executor")]
use async_std::task::JoinHandle;
use commit::Committable;
use core::time::Duration;
use either::Either;
use either::Right;
use futures::future::BoxFuture;
use futures::FutureExt;
use hotshot_consensus::utils::Terminator;
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
use hotshot_task::task_launcher::TaskRunner;
use hotshot_types::data::ProposalType;
use hotshot_types::data::ViewNumber;
use hotshot_types::message::Message;
use hotshot_types::message::Proposal;
use hotshot_types::traits::election::ConsensusExchange;
use hotshot_types::traits::election::QuorumExchangeType;
use hotshot_types::traits::node_implementation::{NodeImplementation, SequencingExchangesType};
use hotshot_types::traits::state::ConsensusTime;
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
use nll::nll_todo::nll_todo;
use snafu::Snafu;
use std::alloc::GlobalAlloc;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
#[cfg(feature = "tokio-executor")]
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

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
    /// The High QC.
    pub high_qc: QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,

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
    pub vote_collector: Option<(ViewNumber, usize)>,

    /// timeout task handle
    pub timeout_task: JoinHandle<()>,

    /// Global events stream to publish events
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,

    /// All the DA certs we've received for current and future views.
    pub certs: HashMap<ViewNumber, DACertificate<TYPES>>,

    /// The most recent proposal we have, will correspond to the current view if Some()
    /// Will be none if the view advanced through timeout/view_sync
    pub current_proposal: Option<QuorumProposal<TYPES, I::Leaf>>,
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
    // TODO ED Emit a view change event upon new proposal?
    match event {
        SequencingHotShotEvent::QuorumVoteRecv(vote) => match vote {
            QuorumVote::Yes(vote) => {
                // error!("In vote handle");

                // For the case where we receive votes after we've made a certificate
                if state.accumulator.is_right() {
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
                        // state
                        //     .event_stream
                        //     .publish(SequencingHotShotEvent::ViewChange(ViewNumber::new(*state.cur_view + 1)))
                        //     .await;
                        error!("QCFormed!");
                        state
                            .event_stream
                            .publish(SequencingHotShotEvent::QCFormed(qc.clone()))
                            .await;
                        state.accumulator = Either::Right(qc);
                        return (None, state);
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
    async fn send_da(&self) {
        // TODO bf we need to send a DA proposal as soon as we are chosen as the leader
        // ED: Added this in the DA task ^
    }
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
    async fn vote_if_able(&self) {
        if let Some(proposal) = &self.current_proposal {
            // ED Need to account for the genesis DA cert
            if proposal.justify_qc.is_genesis() {
                let view = TYPES::Time::new(0);
                let vote_token = self.quorum_exchange.make_vote_token(view);
                match vote_token {
                    Err(e) => {
                        error!("Failed to generate vote token for {:?} {:?}", view, e);
                    }
                    Ok(None) => {
                        info!("We were not chosen for consensus committee on {:?}", view);
                    }
                    Ok(Some(vote_token)) => {
                        let message: GeneralConsensusMessage<TYPES, I>;
                        message = self.quorum_exchange.create_yes_message(
                            proposal.justify_qc.commit(),
                            proposal.justify_qc.leaf_commitment,
                            view,
                            vote_token,
                        );

                        if let GeneralConsensusMessage::Vote(vote) = message {
                            // error!("Sending vote to next leader {:?}", vote);
                            self.event_stream
                                .publish(SequencingHotShotEvent::QuorumVoteSend(vote))
                                .await;
                        };
                    }
                }
                return;
            }

            if let Some(cert) = self.certs.get(&proposal.get_view_number()) {
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
                        let message: GeneralConsensusMessage<TYPES, I>;

                        // Validate the DAC.
                        if !self
                            .committee_exchange
                            .is_valid_cert(&cert, proposal.block_commitment)
                        {
                            warn!("Invalid DAC in proposal! Skipping proposal.");

                            message = self.quorum_exchange.create_no_message(
                                proposal.justify_qc.commit(),
                                proposal.justify_qc.leaf_commitment,
                                cert.view_number,
                                vote_token,
                            );
                        } else {
                            message = self.quorum_exchange.create_yes_message(
                                proposal.justify_qc.commit(),
                                proposal.justify_qc.leaf_commitment,
                                cert.view_number,
                                vote_token,
                            );
                        }

                        if let GeneralConsensusMessage::Vote(vote) = message {
                            // error!("Sending vote to next leader {:?}", vote);
                            self.event_stream
                                .publish(SequencingHotShotEvent::QuorumVoteSend(vote))
                                .await;
                        };
                    }
                }
            }
        }
    }
    async fn update_view(&mut self, new_view: ViewNumber) {
        // Remove old certs, we won't vote on past views
        for view in *self.cur_view..*new_view - 1 {
            let v = ViewNumber::new(view);
            self.certs.remove(&v);
        }
        self.cur_view = new_view;
        self.current_proposal = None;
    }
    pub async fn handle_event(&mut self, event: SequencingHotShotEvent<TYPES, I>) {
        match event {
            SequencingHotShotEvent::QuorumProposalRecv(proposal, sender) => {
                let view = proposal.data.get_view_number();
                if view < self.cur_view {
                    return;
                }

                let view_leader_key = self.quorum_exchange.get_leader(view);
                if view_leader_key != sender {
                    return;
                }

                // self.update_view(view).await;
                // error!("After {:?}  sender: {:?}", *view, sender);

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

                        let Some(parent) = parent else {
                            warn!("Proposal's parent missing from storage");
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
                                view,
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
                                view,
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
                        // self.update_view(view);

                        // GC only if we are not in the genesis view
                        // ED Should use the genesis() function here
                        if *self.cur_view != 0 {
                            for view in *self.cur_view..*view - 1 {
                                let v = TYPES::Time::new(view);
                                self.certs.remove(&v);
                            }
                        }
                        self.cur_view = view;

                        // ED We already send the vote below
                        self.vote_if_able().await;
                        self.current_proposal = None;

                        self.timeout_task = async_spawn({
                            // let next_view_timeout = hotshot.inner.config.next_view_timeout;
                            // let next_view_timeout = next_view_timeout;
                            // let hotshot: HotShot<TYPES::ConsensusType, TYPES, I> = hotshot.clone();
                            // TODO(bf): get the real timeout from the config.
                            let stream = self.event_stream.clone();
                            let view_number = self.cur_view.clone();
                            async move {
                                async_sleep(Duration::from_millis(10000)).await;
                                stream
                                    .publish(SequencingHotShotEvent::Timeout(ViewNumber::new(
                                        *view_number,
                                    )))
                                    .await;
                            }
                        });

                        // Because we call the vote if able function above
                        // if let GeneralConsensusMessage::Vote(vote) = message {
                        //     info!("Sending vote to next leader {:?}", vote);
                        //     error!("Vote is {:?}", vote.current_view());

                        //     self.event_stream
                        //         .publish(SequencingHotShotEvent::QuorumVoteSend(vote))
                        //         .await;
                        // };
                    }
                }
            }
            SequencingHotShotEvent::QuorumVoteRecv(vote) => {
                match vote {
                    QuorumVote::Yes(vote) => {
                        let handle_event = HandleEvent(Arc::new(move |event, state| {
                            async move { vote_handle(state, event).await }.boxed()
                        }));
                        let collection_view = if let Some((collection_view, collection_task)) =
                            &self.vote_collector
                        {
                            if vote.current_view > *collection_view {
                                self.registry.shutdown_task(*collection_task).await;
                            }
                            collection_view.clone()
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
                            vote.vote_data,
                            vote.vote_token.clone(),
                            vote.current_view,
                            acc,
                            None,
                        );

                        if vote.current_view > collection_view
                            || vote.current_view == ViewNumber::new(0)
                        {
                            // error!("HERE");
                            let state = VoteCollectionTaskState {
                                quorum_exchange: self.quorum_exchange.clone(),
                                accumulator,
                                cur_view: vote.current_view,
                                event_stream: self.event_stream.clone(),
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
                            let _task = async_spawn(async move {
                                VoteCollectionTypes::build(builder).launch().await
                            });
                            self.vote_collector = Some((vote.current_view, id));
                        }
                    }
                    QuorumVote::Timeout(_) | QuorumVote::No(_) => {
                        warn!("The next leader has received an unexpected vote!");
                    }
                }
            }
            SequencingHotShotEvent::QCFormed(qc) => {
                
                // ED Need to update the view here?  What does otherwise?
                // So we don't create a QC on the first view unless we are the leader
                if !self.quorum_exchange.is_leader(self.cur_view) {
                    return;
                }

                // update our high qc to the qc we just formed
                self.high_qc = qc;
                let parent_view_number = &self.high_qc.view_number();
                let consensus = self.consensus.read().await;
                let mut reached_decided = false;

                let Some(parent_view) = consensus.state_map.get(parent_view_number) else {
                    warn!("Couldn't find high QC parent in state map.");
                    return;
                };
                let Some(leaf) = parent_view.get_leaf_commitment() else {
                    warn!(
                        ?parent_view_number,
                        ?parent_view,
                        "Parent of high QC points to a view without a proposal"
                    );
                    return;
                };
                let Some(leaf) = consensus.saved_leaves.get(&leaf) else {
                    warn!("Failed to find high QC parent.");
                    return;
                };
                if leaf.view_number == consensus.last_decided_view {
                    reached_decided = true;
                }
                let parent_leaf = leaf.clone();

                let original_parent_hash = parent_leaf.commit();

                let mut next_parent_hash = original_parent_hash;

                if !reached_decided {
                    while let Some(next_parent_leaf) = consensus.saved_leaves.get(&next_parent_hash)
                    {
                        if next_parent_leaf.view_number <= consensus.last_decided_view {
                            break;
                        }
                        next_parent_hash = next_parent_leaf.parent_commitment;
                    }
                    // TODO do some sort of sanity check on the view number that it matches decided
                }

                let block_commitment = self.block.commit();
                let leaf = SequencingLeaf {
                    view_number: self.cur_view,
                    height: parent_leaf.height + 1,
                    justify_qc: self.high_qc.clone(),
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
                    justify_qc: self.high_qc.clone(),
                    proposer_id: leaf.proposer_id,
                    dac: None,
                };

                let message = Proposal {
                    data: proposal,
                    signature,
                };
                self.event_stream
                    .publish(SequencingHotShotEvent::QuorumProposalSend(
                        message,
                        self.quorum_exchange.public_key().clone(),
                    ))
                    .await;
            }
            SequencingHotShotEvent::DACRecv(cert) => {
                let view = cert.view_number;
                self.certs.insert(view, cert);
                if view == self.cur_view {
                    self.vote_if_able().await;
                }
            }

            SequencingHotShotEvent::ViewChange(new_view) => {
                // update the view in state to the one in the message
                self.update_view(new_view);
                // TODO ED Launch leader and next leader tasks in the case where a QC was not formed
            }
            SequencingHotShotEvent::Timeout(view) => {
                // The view sync module will handle updating views in the case of timeout
                // TODO ED In the future send a timeout vote
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
    match event {
        SequencingHotShotEvent::QuorumProposalRecv(_, _)
        | SequencingHotShotEvent::QuorumVoteRecv(_)
        | SequencingHotShotEvent::QCFormed(_)
        | SequencingHotShotEvent::DACRecv(_)
        | SequencingHotShotEvent::ViewChange(_)
        | SequencingHotShotEvent::Timeout(_) => true,
        _ => false,
    }
}
