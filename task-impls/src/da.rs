use crate::events::SequencingHotShotEvent;
use async_compatibility_layer::art::async_spawn;
use async_lock::RwLock;
#[cfg(feature = "async-std-executor")]
use async_std::task::JoinHandle;
use commit::Committable;
use either::Either;
use either::{Left, Right};
use futures::FutureExt;
use hotshot_consensus::Consensus;
use hotshot_task::event_stream::ChannelStream;
use hotshot_task::event_stream::EventStream;
use hotshot_task::global_registry::GlobalRegistry;
use hotshot_task::task::FilterEvent;
use hotshot_task::task::{HandleEvent, HotShotTaskCompleted, HotShotTaskTypes, TaskErr, TS};
use hotshot_task::task_impls::HSTWithEvent;
use hotshot_task::task_impls::TaskBuilder;
use hotshot_types::certificate::QuorumCertificate;
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
            // TODO ED Add transaction handling logic, looks like there isn't anywhere where DA proposals are created (e.g. create_da_proposal())
            /* What should that logic look like?

            Want the DA proposer to propose once they have enough transactions or a certain amount of time has passed
            How to translate that into events? --> Will assume view number is associated

            See QuorumProposalRecv(view n) --> start building new DA Proposal (view n + 1) (with transactions you already have or wait for more)

            */
            SequencingHotShotEvent::TransactionRecv(transaction) => {
                panic!("Received tx in DA task!");
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
            SequencingHotShotEvent::ViewChange(view) => {
                self.cur_view = view;

                // If we are not the next leader (DA leader for this view) immediately exit
                if self.committee_exchange.get_leader(self.cur_view + 1)
                    != self.committee_exchange.public_key().clone()
                {
                    return None;
                }

                // ED Copy of parent_leaf() function from sequencing leader

                let parent_view_number = &self.high_qc.view_number;
                // let consensus = self.consensus.read().await;
                // let Some(parent_view) = consensus.state_map.get(parent_view_number) else {
                //     warn!("Couldn't find high QC parent in state map.");
                //     return None;
                // };
                // let Some(leaf) = parent_view.get_leaf_commitment() else {
                //     warn!(
                //         ?parent_view_number,
                //         ?parent_view,
                //         "Parent of high QC points to a view without a proposal"
                //     );
                //     return None;
                // };
                // let Some(leaf) = consensus.saved_leaves.get(&leaf) else {
                //     warn!("Failed to find high QC parent.");
                //     return None;
                // };
                // Some(leaf.clone())

                // Prepare the DA Proposal
                //         let Some(parent_leaf) = self.parent_leaf().await else {
                //     warn!("Couldn't find high QC parent in state map.");
                //     return None;
                // };

                //         let mut block = <TYPES as NodeType>::StateType::next_block(None);
                //         let txns = self.wait_for_transactions().await?;

                //         for txn in txns {
                //             if let Ok(new_block) = block.add_transaction_raw(&txn) {
                //                 block = new_block;
                //                 continue;
                //             }
                //         }
                //         let block_commitment = block.commit();

                //         let consensus = self.consensus.read().await;
                //         let signature = self.committee_exchange.sign_da_proposal(&block.commit());
                //         let data: DAProposal<TYPES> = DAProposal {
                //             deltas: block.clone(),
                //             view_number: self.cur_view,
                //         };
                //         let message = SequencingMessage::<TYPES, I>(Right(
                //             CommitteeConsensusMessage::DAProposal(Proposal { data, signature }),
                //         ));
                // Brodcast DA proposal
                // TODO ED Send event

                return None;
            }
            SequencingHotShotEvent::Shutdown => {
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
