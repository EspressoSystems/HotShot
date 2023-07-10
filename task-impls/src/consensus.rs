use crate::events::SequencingHotShotEvent;
use async_compatibility_layer::art::{async_sleep, async_spawn};

use async_lock::RwLock;
use async_lock::RwLockUpgradableReadGuard;
#[cfg(feature = "async-std-executor")]
use async_std::task::JoinHandle;
use commit::Commitment;
use commit::Committable;
use core::time::Duration;
use either::Either;
use either::Left;
use either::Right;
use futures::future::BoxFuture;
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
use hotshot_task::task_launcher::TaskRunner;
use hotshot_types::data::LeafType;
use hotshot_types::data::ProposalType;
use hotshot_types::data::ViewNumber;
use hotshot_types::message::Message;
use hotshot_types::message::Proposal;
use hotshot_types::traits::election::ConsensusExchange;
use hotshot_types::traits::election::QuorumExchangeType;
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
use snafu::Snafu;
use std::alloc::GlobalAlloc;
use std::collections::HashMap;
use std::collections::HashSet;
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

    /// Have we already sent a proposal for a particular view
    /// since proposal can be sent either on QCFormed event or ViewChange event
    // pub proposal_sent: HashMap<TYPES::Time, bool>,

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
                error!("In vote handle with vote view: {}", *vote.current_view);

                // For the case where we receive votes after we've made a certificate
                if state.accumulator.is_right() {
                    error!("Already made qc");
                    return (None, state);
                }

                // error!(
                //     "Vote leaf commitment is: {:?}",
                //     vote.leaf_commitment.clone()
                // );

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
                        let total_votes = acc.total_vote_outcomes.len();
                        error!("Not enough votes total votes: {}", total_votes);
                        state.accumulator = Either::Left(acc);
                        return (None, state);
                    }
                    Either::Right(qc) => {
                        // state
                        //     .event_stream
                        //     .publish(SequencingHotShotEvent::ViewChange(ViewNumber::new(*state.cur_view + 1)))
                        //     .await;
                        error!("QCFormed! {:?}", qc.view_number);
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
                panic!("The next leader has received an unexpected vote!");
                return (None, state);
            }
            QuorumVote::No(_) => {
                panic!("The next leader has received an unexpected vote!");
            }
        },
        SequencingHotShotEvent::Shutdown => {
            error!("Shutting down vote handle");
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
    async fn send_da(&self) {
        // TODO bf we need to send a DA proposal as soon as we are chosen as the leader
        // ED: Added this in the DA task ^
    }
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
    async fn vote_if_able(&self) -> bool {
        error!("In vote if able");

        if let Some(proposal) = &self.current_proposal {
            // ED Need to account for the genesis DA cert
            // // error!("in vote if able for proposal view {:?}", proposal.view_number);

            if proposal.justify_qc.is_genesis() && proposal.view_number == ViewNumber::new(1) {
                // error!("Proposal is genesis!");

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

                        let message: GeneralConsensusMessage<TYPES, I>;
                        message = self.quorum_exchange.create_yes_message(
                            proposal.justify_qc.commit(),
                            leaf.commit(),
                            view,
                            vote_token,
                        );

                        if let GeneralConsensusMessage::Vote(vote) = message {
                            error!("Sending vote to next leader {:?}", vote.current_view());
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
            if let Some(cert) = self.certs.get(&(*&proposal.get_view_number() - 1)) {
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
                        let message: GeneralConsensusMessage<TYPES, I>;

                        // Validate the DAC.
                        if !self
                            .committee_exchange
                            .is_valid_cert(&cert, proposal.block_commitment)
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
                            message = self.quorum_exchange.create_yes_message(
                                proposal.justify_qc.commit(),
                                leaf.commit(),
                                cert.view_number,
                                vote_token,
                            );
                        }

                        if let GeneralConsensusMessage::Vote(vote) = message {
                            error!("Sending vote to next leader {:?}", vote.current_view());
                            self.event_stream
                                .publish(SequencingHotShotEvent::QuorumVoteSend(vote))
                                .await;
                            return true;
                        }
                    }
                }
            }
            error!(
                "Couldn't find DAC cert in certs, meaning we haven't received it yet for view {}",
                *self.cur_view
            );
            return false;
        }
        error!(
            "Could not vote because we don't have a proposal yet for view {}",
            *self.cur_view
        );
        return false;
    }

    /// Must only update the view and GC if the view actually changes
    async fn update_view(&mut self, new_view: ViewNumber) -> bool {
        if *self.cur_view < *new_view {
            error!("Updating view from {} to {}", *self.cur_view, *new_view);

            // Remove old certs, we won't vote on past views
            // TODO ED Put back in once we fix other errors
            // for view in *self.cur_view..*new_view - 1 {
            //     let v = ViewNumber::new(view);
            //     self.certs.remove(&v);
            // }
            self.cur_view = new_view;
            self.current_proposal = None;
            return true;
        }
        false
    }
    pub async fn handle_event(&mut self, event: SequencingHotShotEvent<TYPES, I>) {
        match event {
            SequencingHotShotEvent::QuorumProposalRecv(proposal, sender) => {
                error!("Receved Quorum Propsoal");

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

                // error!("Current view: {:?}", self.cur_view);

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
                        // error!("Leaf replica is voting on! {:?}", leaf.commit());
                        let justify_qc_commitment = justify_qc.commit();
                        let leaf_commitment = leaf.commit();

                        // Validate the `justify_qc`.
                        if !self
                            .quorum_exchange
                            .is_valid_cert(&justify_qc, parent_commitment)
                        {
                            error!("Invalid justify_qc in proposal!.");

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

                        self.high_qc = leaf.justify_qc.clone();

                        //             let mut new_anchor_view = consensus.last_decided_view;
                        //             let mut new_locked_view = consensus.locked_view;
                        //             let mut last_view_number_visited = self.cur_view;
                        //             let mut new_commit_reached: bool = false;
                        //             let mut new_decide_reached = false;
                        //             let mut new_decide_qc = None;
                        //             let mut leaf_views = Vec::new();
                        //             let mut included_txns = HashSet::new();
                        //             let old_anchor_view = consensus.last_decided_view;
                        //             let parent_view = leaf.justify_qc.view_number;
                        //             let mut current_chain_length = 0usize;
                        //             if parent_view + 1 == self.cur_view {
                        //                 current_chain_length += 1;
                        //                 if let Err(e) = consensus.visit_leaf_ancestors(
                        //     parent_view,
                        //     Terminator::Exclusive(old_anchor_view),
                        //     true,
                        //     |leaf| {
                        //         if !new_decide_reached {
                        //             if last_view_number_visited == leaf.view_number + 1 {
                        //                 last_view_number_visited = leaf.view_number;
                        //                 current_chain_length += 1;
                        //                 if current_chain_length == 2 {
                        //                     new_locked_view = leaf.view_number;
                        //                     new_commit_reached = true;
                        //                     // The next leaf in the chain, if there is one, is decided, so this
                        //                     // leaf's justify_qc would become the QC for the decided chain.
                        //                     new_decide_qc = Some(leaf.justify_qc.clone());
                        //                 } else if current_chain_length == 3 {
                        //                     new_anchor_view = leaf.view_number;
                        //                     new_decide_reached = true;
                        //                 }
                        //             } else {
                        //                 // nothing more to do here... we don't have a new chain extension
                        //                 return false;
                        //             }
                        //         }
                        //         // starting from the first iteration with a three chain, e.g. right after the else if case nested in the if case above
                        //         if new_decide_reached {
                        //             let mut leaf = leaf.clone();

                        //             // If the full block is available for this leaf, include it in the leaf
                        //             // chain that we send to the client.
                        //             if let Some(block) =
                        //                 consensus.saved_blocks.get(leaf.get_deltas_commitment())
                        //             {
                        //                 if let Err(err) = leaf.fill_deltas(block.clone()) {
                        //                     warn!("unable to fill leaf {} with block {}, block will not be available: {}",
                        //                         leaf.commit(), block.commit(), err);
                        //                 }
                        //             }

                        //             leaf_views.push(leaf.clone());
                        //             if let Left(block) = &leaf.deltas {
                        //                 let txns = block.contained_transactions();
                        //                 for txn in txns {
                        //                     included_txns.insert(txn);
                        //                 }
                        //             }
                        //         }
                        //         true
                        //     },
                        // ) {
                        //     self.api.send_view_error(self.cur_view, Arc::new(e)).await;
                        // }
                        //             }

                        //             let included_txns_set: HashSet<_> = if new_decide_reached {
                        //                 included_txns
                        //             } else {
                        //                 HashSet::new()
                        //             };

                        // promote lock here to add proposal to statemap
                        let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;
                        consensus.state_map.insert(
                            self.cur_view,
                            View {
                                view_inner: ViewInner::Leaf {
                                    leaf: leaf.commit(),
                                },
                            },
                        );
                        // error!("Inserting leaf into storage {:?}", leaf.commit());
                        consensus.saved_leaves.insert(leaf.commit(), leaf.clone());

                        drop(consensus);
                        if !self.vote_if_able().await {
                            return;
                        }

                        // ED Only do this GC if we are able to vote
                        for view in *self.cur_view..*view - 1 {
                            let v = TYPES::Time::new(view);
                            self.certs.remove(&v);
                        }
                        // error!("Voting for view {}", *self.cur_view);
                        // self.current_proposal = None;
                        let new_view = self.current_proposal.clone().unwrap().view_number + 1;

                        // Update current view and publish a view change event so other tasks also update
                        self.update_view(new_view).await;
                        self.event_stream
                            .publish(SequencingHotShotEvent::ViewChange(self.cur_view))
                            .await;

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

                        if let GeneralConsensusMessage::Vote(vote) = message {
                            info!("Sending vote to next leader {:?}", vote);
                            error!("Vote is {:?}", vote.current_view());

                            self.event_stream
                                .publish(SequencingHotShotEvent::QuorumVoteSend(vote))
                                .await;
                        };
                    }
                }
            }
            SequencingHotShotEvent::QuorumVoteRecv(vote) => {
                match vote {
                    QuorumVote::Yes(vote) => {
                        error!(
                            "Recved quorum vote outside of vote handle for view {:?}",
                            vote.current_view
                        );
                        let handle_event = HandleEvent(Arc::new(move |event, state| {
                            async move { vote_handle(state, event).await }.boxed()
                        }));
                        let collection_view = if let Some((collection_view, collection_task)) =
                            &self.vote_collector
                        {
                            if vote.current_view > *collection_view {
                                // ED I think we'd want to let that task timeout to avoid a griefing vector
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

                        if vote.current_view > collection_view {
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

                            self.vote_collector = Some((vote.current_view, id));

                            let _task = async_spawn(async move {
                                VoteCollectionTypes::build(builder).launch().await;
                            });
                            error!("Starting vote handle for view {:?}", vote.current_view);
                        }
                    }
                    QuorumVote::Timeout(_) | QuorumVote::No(_) => {
                        panic!("The next leader has received an unexpected vote!");
                    }
                }
            }
            SequencingHotShotEvent::QCFormed(qc) => {
                error!("QC Formed event happened!");

                self.high_qc = qc.clone();
                // error!("QC leaf commitment is {:?}", qc.leaf_commitment());
                // self.event_stream
                //     .publish(SequencingHotShotEvent::ViewChange(qc.view_number() + 1))
                //     .await;

                // View may have already been updated by replica if they voted for this QC
                // TODO ED We should separate leader state from replica state, they shouldn't share the same view
                // Leader task should only run for a specific view, and never update its current view, but instead spawn another task
                let _res = self.update_view(qc.view_number + 1).await;
                // error!("Handle qc formed event!");

                // // So we don't create a QC on the first view unless we are the leader
                if !self.quorum_exchange.is_leader(self.cur_view) {
                    return;
                }

                // update our high qc to the qc we just formed
                // self.high_qc = qc;
                let parent_view_number = &self.high_qc.view_number();
                // error!("Parent view number is {:?}", parent_view_number);
                let consensus = self.consensus.read().await;
                let mut reached_decided = false;

                let Some(parent_view) = consensus.state_map.get(parent_view_number) else {
                    // This should have been added by the replica? 
                    error!("Couldn't find parent view in state map.");
                    return;
                };
                // Leaf hash in view inner does not match high qc hash - Why?
                let Some(leaf_commitment) = parent_view.get_leaf_commitment() else {
                    error!(
                        ?parent_view_number,
                        ?parent_view,
                        "Parent of high QC points to a view without a proposal"
                    );
                    return;
                };
                if leaf_commitment != self.high_qc.leaf_commitment() {
                    // error!(
                    //     "They don't equal: {:?}   {:?}",
                    //     leaf_commitment,
                    //     self.high_qc.leaf_commitment()
                    // );
                }
                let Some(leaf) = consensus.saved_leaves.get(&leaf_commitment) else {
                    error!("Failed to find high QC of parent.");
                    return;
                };
                if leaf.view_number == consensus.last_decided_view {
                    reached_decided = true;
                }

                let parent_leaf = leaf.clone();

                let original_parent_hash = parent_leaf.commit();

                let mut next_parent_hash = original_parent_hash;

                // Walk back until we find a decide
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
                if block_commitment == TYPES::BlockType::new().commit() {
                    error!("Block is generic block! {:?}", self.cur_view);
                }
                // error!(
                //     "leaf commitment of new qc: {:?}",
                //     self.high_qc.leaf_commitment()
                // );
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
                // error!("Leaf sent in proposal! {:?}", parent_leaf.commit());

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
                // error!("Sending proposal for view {:?} \n {:?}", self.cur_view, message.clone());
                error!("Sending proposal for view {:?}", self.cur_view);

                self.event_stream
                    .publish(SequencingHotShotEvent::QuorumProposalSend(
                        message,
                        self.quorum_exchange.public_key().clone(),
                    ))
                    .await;
            }
            SequencingHotShotEvent::DACRecv(cert) => {
                error!("DAC Recved for view ! {}", *cert.view_number);

                let view = cert.view_number;
                self.certs.insert(view, cert);
                if view == self.cur_view {
                    self.vote_if_able().await;
                }
            }

            SequencingHotShotEvent::ViewChange(new_view) => {
                error!("View Change event for view {}", *new_view);

                // update the view in state to the one in the message
                // ED Update_view return a bool whether it actually updated
                if !self.update_view(new_view).await {
                    return;
                }

                // ED Need to update the view here?  What does otherwise?
                // self.update_view(qc.view_number + 1).await;
                // So we don't create a QC on the first view unless we are the leader
                if !self.quorum_exchange.is_leader(self.cur_view) {
                    return;
                }

                // update our high qc to the qc we just formed
                // self.high_qc = qc;
                let parent_view_number = &self.high_qc.view_number();
                let consensus = self.consensus.read().await;
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
                // error!("Sending proposal for view {:?} \n {:?}", self.cur_view, message.clone());
                error!("Sending proposal for view {:?}", self.cur_view);

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
                error!("We received a timeout event in the consensus task!")
            }
            SequencingHotShotEvent::SendDABlockData(block) => {
                // ED TODO Should make sure this is actually the most recent block
                error!("Updating self . block!");
                self.block = block;
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
        | SequencingHotShotEvent::SendDABlockData(_)
        | SequencingHotShotEvent::Timeout(_) => true,
        _ => false,
    }
}
