use crate::events::SequencingHotShotEvent;
use async_compatibility_layer::art::async_spawn;
use async_compatibility_layer::art::async_timeout;
use async_compatibility_layer::async_primitives::subscribable_rwlock::ReadView;
use async_compatibility_layer::async_primitives::subscribable_rwlock::SubscribableRwLock;
use async_lock::RwLock;
#[cfg(feature = "async-std-executor")]
use async_std::task::JoinHandle;
use commit::Commitment;
use commit::Committable;
use either::Either;
use either::{Left, Right};
use futures::FutureExt;
use hotshot_consensus::Consensus;
use hotshot_consensus::SequencingConsensusApi;
use hotshot_task::event_stream::ChannelStream;
use hotshot_task::event_stream::EventStream;
use hotshot_task::global_registry::GlobalRegistry;
use hotshot_task::task::FilterEvent;
use hotshot_task::task::{HandleEvent, HotShotTaskCompleted, HotShotTaskTypes, TaskErr, TS};
use hotshot_task::task_impls::HSTWithEvent;
use hotshot_task::task_impls::TaskBuilder;
use hotshot_types::certificate::QuorumCertificate;
use hotshot_types::data::DAProposal;
use hotshot_types::message::Proposal;
use hotshot_types::message::{CommitteeConsensusMessage, Message};
use hotshot_types::traits::election::{CommitteeExchangeType, ConsensusExchange};
use hotshot_types::traits::node_implementation::{NodeImplementation, SequencingExchangesType};
use hotshot_types::traits::Block;
use hotshot_types::traits::State;
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
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
#[cfg(feature = "tokio-executor")]
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

/// A type alias for `HashMap<Commitment<T>, T>`
type CommitmentMap<T> = HashMap<Commitment<T>, T>;

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
    A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + 'static,
> where
    I::Exchanges: SequencingExchangesType<TYPES, Message<TYPES, I>>,
    CommitteeEx<TYPES, I>: ConsensusExchange<
        TYPES,
        Message<TYPES, I>,
        Certificate = DACertificate<TYPES>,
        Commitment = TYPES::BlockType,
    >,
{
    pub api: A,
    pub registry: GlobalRegistry,

    /// View number this view is executing in.
    pub cur_view: ViewNumber,

    // pub transactions: Arc<SubscribableRwLock<CommitmentMap<TYPES::Transaction>>>,
    /// Reference to consensus. Leader will require a read lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES, SequencingLeaf<TYPES>>>>,

    /// The High QC.
    pub high_qc: QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,

    /// the committee exchange
    pub committee_exchange: Arc<CommitteeEx<TYPES, I>>,

    /// The view and ID of the current vote collection task, if there is one.
    pub vote_collector: Option<(ViewNumber, usize)>,

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
        A: SequencingConsensusApi<TYPES, SequencingLeaf<TYPES>, I> + 'static,
    > DATaskState<TYPES, I, A>
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
            // TODO ED Add transaction handling logic, looks like there isn't anywhere where DA proposals are created (e.g. create_da_proposal())
            /* What should that logic look like?

            Want the DA proposer to propose once they have enough transactions or a certain amount of time has passed
            How to translate that into events? --> Will assume view number is associated

            See QuorumProposalRecv(view n) --> start building new DA Proposal (view n + 1) (with transactions you already have or wait for more)

            */
            SequencingHotShotEvent::TransactionRecv(transaction) => {
                // error!("Received tx in DA task!");
                // TODO ED Add validation checks
                self.consensus.read().await.get_transactions()
                    .modify(|txns| {
                        let _new = txns.insert(transaction.commit(), transaction).is_none();
                    })
                    .await;
                return None;
            }
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
                let collection_view =
                    if let Some((collection_view, collection_id)) = &self.vote_collector {
                        if view > *collection_view {
                            self.registry.shutdown_task(*collection_id).await;
                        }
                        collection_view.clone()
                    } else {
                        ViewNumber::new(0)
                    };
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
                if view > collection_view {
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
                    self.vote_collector = Some((view, id));
                }
            }
            // TODO ED Update high QC through QCFormed event
            SequencingHotShotEvent::ViewChange(view) => {
                self.cur_view = view;

                // TODO ED Make this a new task so it doesn't block main DA task

                // If we are not the next leader (DA leader for this view) immediately exit
                if self.committee_exchange.get_leader(self.cur_view + 1)
                    != self.committee_exchange.public_key().clone()
                {
                    return None;
                }

                // ED Copy of parent_leaf() function from sequencing leader

                let parent_view_number = &self.high_qc.view_number;
                let consensus = self.consensus.read().await;
                let Some(parent_view) = consensus.state_map.get(parent_view_number) else {
                    warn!("Couldn't find high QC parent in state map.");
                    return None;
                };
                let Some(leaf) = parent_view.get_leaf_commitment() else {
                    warn!(
                        ?parent_view_number,
                        ?parent_view,
                        "Parent of high QC points to a view without a proposal"
                    );
                    return None;
                };
                let Some(leaf) = consensus.saved_leaves.get(&leaf) else {
                    warn!("Failed to find high QC parent.");
                    return None;
                };
                let parent_leaf = leaf.clone();

                // Prepare the DA Proposal
                //         let Some(parent_leaf) = self.parent_leaf().await else {
                //     warn!("Couldn't find high QC parent in state map.");
                //     return None;
                // };

                let mut block = <TYPES as NodeType>::StateType::next_block(None);
                let txns = self.wait_for_transactions(parent_leaf).await?;

                for txn in txns {
                    if let Ok(new_block) = block.add_transaction_raw(&txn) {
                        block = new_block;
                        continue;
                    }
                }
                let block_commitment = block.commit();

                let consensus = self.consensus.read().await;
                let signature = self.committee_exchange.sign_da_proposal(&block.commit());
                let data: DAProposal<TYPES> = DAProposal {
                    deltas: block.clone(),
                    view_number: self.cur_view,
                };
                let message = SequencingMessage::<TYPES, I>(Right(
                    CommitteeConsensusMessage::DAProposal(Proposal { data, signature }),
                ));
                // Brodcast DA proposal
                // TODO ED We should send an event to do this, but just getting it to work for now
                if let Err(e) = self.api.send_da_broadcast(message.clone()).await {
                    consensus.metrics.failed_to_send_messages.add(1);
                    warn!(?message, ?e, "Could not broadcast leader proposal");
                } else {
                    consensus.metrics.outgoing_broadcast_messages.add(1);
                }

                return None;
            }
            SequencingHotShotEvent::Shutdown => {
                return Some(HotShotTaskCompleted::ShutDown);
            }
            _ => {}
        }
        None
    }

    /// return None if we can't get transactions
    async fn wait_for_transactions(
        &self,
        parent_leaf: SequencingLeaf<TYPES>,
    ) -> Option<Vec<TYPES::Transaction>> {
        let task_start_time = Instant::now();

        // let parent_leaf = self.parent_leaf().await?;
        let previous_used_txns = match parent_leaf.deltas {
            Either::Left(block) => block.contained_transactions(),
            Either::Right(_commitment) => HashSet::new(),
        };
        let consensus = self.consensus.read().await;

        let receiver = consensus.transactions.subscribe().await;

        while task_start_time.elapsed() < self.api.propose_max_round_time() {
            let txns = consensus.transactions.cloned().await;
            let unclaimed_txns: Vec<_> = txns
                .iter()
                .filter(|(txn_hash, _txn)| !previous_used_txns.contains(txn_hash))
                .collect();

            let time_past = task_start_time.elapsed();
            if unclaimed_txns.len() < self.api.min_transactions()
                && (time_past < self.api.propose_max_round_time())
            {
                let duration = self.api.propose_max_round_time() - time_past;
                let result = async_timeout(duration, receiver.recv()).await;
                match result {
                    Err(_) => {
                        // Fall through below to updating new block
                        info!("propose_max_round_time passed, sending transactions we have so far");
                    }
                    Ok(Err(e)) => {
                        // Something unprecedented is wrong, and `transactions` has been dropped
                        error!("Channel receiver error for SubscribableRwLock {:?}", e);
                        return None;
                    }
                    Ok(Ok(_)) => continue,
                }
            }
            let mut txns = vec![];
            for (_hash, txn) in unclaimed_txns {
                txns.push(txn.clone());
            }
            return Some(txns);
        }
        None
    }

    /// Filter the DA event.
    pub fn filter(event: &SequencingHotShotEvent<TYPES, I>) -> bool {
        match event {
            SequencingHotShotEvent::DAProposalRecv(_, _)
            | SequencingHotShotEvent::DAVoteRecv(_)
            | SequencingHotShotEvent::Shutdown
            | SequencingHotShotEvent::TransactionRecv(_)
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
    > TS for DATaskState<TYPES, I, A>
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

pub type DATaskTypes<TYPES, I, A> = HSTWithEvent<
    ConsensusTaskError,
    SequencingHotShotEvent<TYPES, I>,
    ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    DATaskState<TYPES, I, A>,
>;
