//! State machine representation of round logic
//!
//! This module provides an implementation of the logic for completing an individual round of
//! sequential [`PhaseLock`](crate::PhaseLock), expressed as a manually written out state machine

#![allow(
    clippy::panic,
    clippy::mut_mut,
    clippy::match_same_arms,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::shadow_unrelated
)] // Clippy hates select and me
use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures::{future::BoxFuture, Future, FutureExt};
use tracing::{info, instrument};

use super::{
    debug, error, generate_qc, trace, warn, yield_now, Arc, Commit, Debug, Decide, Event,
    EventType, Leaf, LeafHash, Message, PhaseLock, PhaseLockError, PreCommit, Prepare, PubKey,
    QuorumCertificate, Result, ResultExt, Stage, Vote,
};
use crate::{
    traits::{
        BlockContents, NetworkingImplementation, State, StatefulHandler, Storage, StorageResult,
    },
    types::error::{FailedToBroadcastSnafu, FailedToMessageLeaderSnafu},
    utility::broadcast::BroadcastSender,
    NodeImplementation,
};

// TODO(nm): state_machine_future is kind of jank, not well documented, and not quite flexible
// enough for this. A lot of this module is boiler plate and can be macroed away. I need to write
// such a macro library.

/// Represents the round logic for sequential [`PhaseLock`]
pub struct SequentialRound<I: NodeImplementation<N> + Send + Sync + 'static, const N: usize> {
    /// State machine
    state: SequentialState<I, N>,
    /// Ref to the [`PhaseLock`] instance
    phaselock: PhaseLock<I, N>,
    /// View number of this round
    current_view: u64,
}

impl<I: NodeImplementation<N> + Send + Sync + 'static, const N: usize> Future
    for SequentialRound<I, N>
where
    Self: Unpin,
{
    type Output = Result<u64>;

    #[instrument(skip(self, cx))]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        debug!("Conducting first poll");
        // Poll the state machine once
        let this = &mut *self;
        let mut res = this.state.poll(cx, &mut this.phaselock, this.current_view);
        // Continue to attempt to transition the state machine while it is signaling to
        let mut x = 1;
        while matches!(res, Poll::Ready(Err(PhaseLockError::Continue))) {
            debug!(?x, "Repolling state machine");
            res = this.state.poll(cx, &mut this.phaselock, this.current_view);
            x += 1;
        }
        res
    }
}

impl<I: NodeImplementation<N> + Send + Sync + 'static, const N: usize> SequentialRound<I, N> {
    /// Creates a new state machine
    pub fn new(
        phaselock: PhaseLock<I, N>,
        current_view: u64,
        channel: Option<&BroadcastSender<Event<I::Block, I::State>>>,
    ) -> Self {
        Self {
            state: SequentialState::Start(channel.cloned()),
            phaselock,
            current_view,
        }
    }
}

/// Context holder
#[derive(Debug, Clone)]
#[allow(dead_code)] // Clippy gets a lil wonk here
struct Ctx<I: NodeImplementation<N> + 'static, const N: usize> {
    /// Current commited state
    committed_state: Arc<I::State>,
    /// Current commited leaf
    committed_leaf: LeafHash<N>,
    /// Handle event stream
    channel: Option<BroadcastSender<Event<I::Block, I::State>>>,
}

impl<I: NodeImplementation<N> + Send + Sync + 'static, const N: usize> Ctx<I, N> {
    /// Generates a context object
    async fn get(
        phaselock: PhaseLock<I, N>,
        channel: Option<BroadcastSender<Event<I::Block, I::State>>>,
    ) -> Self {
        let committed_leaf = *phaselock.inner.committed_leaf.read().await;
        let committed_state = phaselock.inner.committed_state.read().await.clone();
        Self {
            committed_state,
            committed_leaf,
            channel,
        }
    }

    /// sends a proposal event down the channel
    fn send_propose(&self, view_number: u64, block: &I::Block) {
        if let Some(c) = self.channel.as_ref() {
            let _result = c.send(Event {
                view_number,
                stage: Stage::Prepare,
                event: EventType::Propose {
                    block: Arc::new(block.clone()),
                },
            });
        }
    }

    /// sends a decide event down the channel
    fn send_decide(&self, view_number: u64, blocks: &[(I::Block, I::State)]) {
        if let Some(c) = self.channel.as_ref() {
            let _result = c.send(Event {
                view_number,
                stage: Stage::Prepare,
                event: EventType::Decide {
                    block: Arc::new(blocks.iter().map(|x| x.0.clone()).collect()),
                    state: Arc::new(blocks.iter().map(|x| x.1.clone()).collect()),
                },
            });
        }
    }
}

/// Wraps a future to make it `Debug`
struct DebugFuture<'a, T>(BoxFuture<'a, T>);
impl<'a, T> std::fmt::Debug for DebugFuture<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DebugFuture").field(&"Future").finish()
    }
}

/// Represents the current state for a round of Sequential [`PhaseLock`]
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum SequentialState<I: NodeImplementation<N> + 'static, const N: usize> {
    /// Initial starting state of the state machine
    Start(Option<BroadcastSender<Event<I::Block, I::State>>>),
    /// Waiting for context to be built
    Ctx(DebugFuture<'static, Ctx<I, N>>),
    /// This node is a leader
    Leader(SequentialLeader<I, N>, Ctx<I, N>),
    /// This node is a replica
    Replica {
        /// State the replica is in
        state: SequentialReplica<I, N>,
        /// Leader for this round
        leader: PubKey,
        /// Context object
        ctx: Ctx<I, N>,
    },
}

impl<I: NodeImplementation<N> + Send + Sync + 'static, const N: usize> SequentialState<I, N> {
    #[instrument(skip(cx,phaselock), fields(id = phaselock.inner.public_key.nonce))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        phaselock: &mut PhaseLock<I, N>,
        current_view: u64,
    ) -> Poll<Result<u64>> {
        debug!(?self);
        match self {
            SequentialState::Start(chan) => {
                debug!("Setting context future");
                *self = SequentialState::Ctx(DebugFuture(
                    Ctx::get(phaselock.clone(), chan.clone()).boxed(),
                ));
                Poll::Ready(Err(PhaseLockError::Continue))
            }
            SequentialState::Ctx(fut) => {
                debug!("Polling context future");
                match Future::poll(Pin::as_mut(&mut fut.0), cx) {
                    Poll::Ready(ctx) => {
                        debug!("Context was ready");
                        // Determine if we are the leader or not
                        let leader = phaselock.inner.get_leader(current_view);
                        if leader == phaselock.inner.public_key {
                            debug!("Node is leader");
                            *self = SequentialState::Leader(SequentialLeader::Start, ctx);
                        } else {
                            debug!("Node is replica");
                            *self = SequentialState::Replica {
                                state: SequentialReplica::Start,
                                leader,
                                ctx,
                            };
                        }
                        Poll::Ready(Err(PhaseLockError::Continue))
                    }
                    Poll::Pending => {
                        debug!("Context was not ready");
                        Poll::Pending
                    }
                }
            }
            SequentialState::Leader(x, ctx) => x.poll(cx, phaselock, current_view, ctx),
            SequentialState::Replica { state, leader, ctx } => {
                state.poll(cx, phaselock, current_view, leader, ctx)
            }
        }
    }
}

/// Represents the leader logic for a round of Sequential [`PhaseLock`]
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum SequentialLeader<I: NodeImplementation<N> + 'static, const N: usize> {
    /// Initial starting state for A leader
    Start,
    /// Waiting for new-views to come in
    Prepare(DebugFuture<'static, Result<(I::Block, Leaf<I::Block, N>, I::State)>>),
    /// Waiting for prepare votes to come in
    Precommit(DebugFuture<'static, Result<(I::Block, Leaf<I::Block, N>, I::State)>>),
    /// Waiting for precommit votes to come in
    Commit(DebugFuture<'static, Result<(I::Block, Leaf<I::Block, N>, I::State)>>),
    /// Waiting for commit votes to come in
    Decide(DebugFuture<'static, Result<u64>>),
    /// Final state
    End(Result<u64>),
}

impl<I: NodeImplementation<N> + Send + Sync + 'static, const N: usize> SequentialLeader<I, N> {
    #[instrument(skip(cx, phaselock), fields(id = phaselock.inner.public_key.nonce))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        phaselock: &mut PhaseLock<I, N>,
        current_view: u64,
        context: &Ctx<I, N>,
    ) -> Poll<Result<u64>> {
        match self {
            // Setup the PREPARE phase future
            SequentialLeader::Start => {
                let pl = phaselock.clone();
                let ctx = context.clone();
                let fut = async move {
                    // Collect the new-views
                    debug!("Waiting for new views");
                    let new_views = pl.inner.new_view_queue.wait().await;
                    // find the high qc
                    let high_qc = new_views
                        .iter()
                        .max_by_key(|x| x.justify.view_number)
                        .unwrap()
                        .justify
                        .clone();
                    debug!("Found HighQC, getting most recent state");
                    // Find the leaf
                    let leaf = match pl
                        .inner
                        .storage
                        .get_leaf_by_block(&high_qc.block_hash)
                        .await
                    {
                        StorageResult::Some(x) => x,
                        StorageResult::None => {
                            error!(?high_qc, "State not found in storage (by qc)");
                            return Err(PhaseLockError::ItemNotFound {
                                hash: high_qc.block_hash.to_vec(),
                            });
                        }
                        StorageResult::Err(err) => {
                            return Err(PhaseLockError::StorageError { err })
                        }
                    };
                    let leaf_hash = leaf.hash();
                    // Find the state
                    let state = match pl.inner.storage.get_state(&leaf_hash).await {
                        StorageResult::Some(x) => x,
                        StorageResult::None => {
                            error!(?leaf_hash, "State not found in storage (by leaf)");
                            return Err(PhaseLockError::ItemNotFound {
                                hash: leaf_hash.to_vec(),
                            });
                        }
                        StorageResult::Err(err) => {
                            return Err(PhaseLockError::StorageError { err })
                        }
                    };
                    trace!(?state, ?leaf_hash);
                    // Prepare our block
                    let mut block = state.next_block();
                    let mut found_txn = false;
                    while !found_txn {
                        // spin while the transaction_queue is empty
                        trace!("Entering spin while we wait for transactions");
                        while pl.inner.transaction_queue.read().await.is_empty() {
                            trace!("executing transaction queue spin cycle");
                            yield_now().await;
                        }
                        debug!("Unloading transactions");
                        let mut transaction_queue = pl.inner.transaction_queue.write().await;
                        // Iterate through all the transactions, keeping the valid ones and
                        // discarding the invalid ones
                        for tx in transaction_queue.drain(..) {
                            // Make sure the transaction is valid given the current state,
                            // otherwise, discard it
                            let new_block = block.add_transaction_raw(&tx);
                            match new_block {
                                Ok(new_block) => {
                                    if state.validate_block(&new_block) {
                                        block = new_block;
                                        found_txn = true;
                                        debug!(?tx, "Added transaction to block");
                                    } else {
                                        let err = state.append(&new_block).unwrap_err();
                                        warn!(?tx, ?err, "Invalid transaction rejected");
                                    }
                                }
                                Err(e) => warn!(?e, ?tx, "Invalid transaction rejected"),
                            }
                        }
                    }
                    // Create new leaf and add it to the store
                    let new_leaf = Leaf::new(block.clone(), high_qc.leaf_hash);
                    let the_hash = new_leaf.hash();
                    pl.inner.storage.insert_leaf(new_leaf.clone()).await;
                    debug!(?new_leaf, ?the_hash, "Leaf created and added to store");
                    // Add resulting state to storage
                    let new_state = state.append(&new_leaf.item).map_err(|error| {
                        error!(?error, "Failed to append block to existing state");
                        PhaseLockError::InconsistentBlock {
                            stage: Stage::Prepare,
                        }
                    })?;
                    // Insert new state into storage
                    debug!(?new_state, "New state inserted");
                    match pl
                        .inner
                        .storage
                        .insert_state(new_state.clone(), the_hash)
                        .await
                    {
                        StorageResult::Some(_) => (),
                        StorageResult::None => (),
                        StorageResult::Err(e) => {
                            return Err(PhaseLockError::StorageError { err: e })
                        }
                    }
                    // Broadcast out the leaf
                    let network_result = pl
                        .inner
                        .networking
                        .broadcast_message(Message::Prepare(Prepare {
                            current_view,
                            leaf: new_leaf.clone(),
                            high_qc: high_qc.clone(),
                            state: new_state.clone(),
                        }))
                        .await
                        .context(FailedToBroadcastSnafu {
                            stage: Stage::Prepare,
                        });
                    if let Err(e) = network_result {
                        warn!(?e, "Error broadcasting leaf");
                    }
                    // Notify our listeners
                    ctx.send_propose(current_view, &block);
                    // Make a prepare signature and send it to ourselves
                    let signature =
                        pl.inner
                            .private_key
                            .partial_sign(&the_hash, Stage::Prepare, current_view);
                    let vote = Vote {
                        signature,
                        leaf_hash: the_hash,
                        id: pl.inner.public_key.nonce,
                        current_view,
                        stage: Stage::Prepare,
                    };
                    pl.inner.prepare_vote_queue.push(vote).await;
                    Ok((block, new_leaf, new_state))
                }
                .boxed();
                *self = SequentialLeader::Prepare(DebugFuture(fut));
                Poll::Ready(Err(PhaseLockError::Continue))
            }
            // Evaluate the PREPARE future and setup the PRECOMMIT future
            SequentialLeader::Prepare(fut) => match Future::poll(fut.0.as_mut(), cx) {
                Poll::Ready(x) => {
                    debug!(?x, "results from the PREPARE future");
                    let (block, new_leaf, state) = x?;
                    let new_leaf_hash = new_leaf.hash();
                    let pl = phaselock.clone();
                    let _ctx = context.clone();
                    let fut = async move {
                        trace!("Waiting for threshold number of votes from nodes");
                        let mut vote_queue = pl
                            .inner
                            .prepare_vote_queue
                            .wait_for(|x| x.current_view == current_view)
                            .await;
                        debug!("Received threshold number of votes");
                        let votes: Vec<_> = vote_queue
                            .drain(..)
                            .filter(|x| x.leaf_hash == new_leaf_hash)
                            .map(|x| (x.id, x.signature))
                            .collect();
                        // Generate the QC
                        let signature = generate_qc(
                            votes.iter().map(|(x, z)| (*x, z)),
                            &pl.inner.public_key.set,
                        )
                        .map_err(|e| {
                            PhaseLockError::FailedToAssembleQC {
                                stage: Stage::PreCommit,
                                source: e,
                            }
                        })?;
                        let qc = QuorumCertificate {
                            block_hash: BlockContents::hash(&block),
                            leaf_hash: new_leaf_hash,
                            signature: Some(signature),
                            stage: Stage::Prepare,
                            view_number: current_view,
                            genesis: false,
                        };
                        debug!(?qc, "Pre-commit qc generated");
                        // Store the precommit qc
                        let mut pqc = pl.inner.prepare_qc.write().await;
                        *pqc = Some(qc.clone());
                        pl.inner.storage.insert_qc(qc.clone()).await;
                        trace!("Pre-commit qc stored in prepare_qc");
                        let pc_message = Message::PreCommit(PreCommit {
                            leaf_hash: new_leaf_hash,
                            qc,
                            current_view,
                        });
                        let network_result = pl
                            .inner
                            .networking
                            .broadcast_message(pc_message)
                            .await
                            .context(FailedToBroadcastSnafu {
                                stage: Stage::PreCommit,
                            });
                        if let Err(e) = network_result {
                            warn!(?e, "Failed to broadcast precommit");
                        }
                        debug!("Precommit message sent");
                        // Make a vote and send it to ourself
                        let signature = pl.inner.private_key.partial_sign(
                            &new_leaf_hash,
                            Stage::PreCommit,
                            current_view,
                        );
                        let vote_message = Vote {
                            leaf_hash: new_leaf_hash,
                            signature,
                            id: pl.inner.public_key.nonce,
                            current_view,
                            stage: Stage::Prepare,
                        };
                        pl.inner.precommit_vote_queue.push(vote_message).await;
                        Ok((block, new_leaf, state))
                    }
                    .boxed();
                    *self = SequentialLeader::Precommit(DebugFuture(fut));
                    Poll::Ready(Err(PhaseLockError::Continue))
                }
                Poll::Pending => Poll::Pending,
            },
            // Evaluate the PRECOMMIT future and setup the COMMIT future
            SequentialLeader::Precommit(fut) => {
                if let Poll::Ready(x) = Future::poll(fut.0.as_mut(), cx) {
                    debug!(?x, "results from PRECOMMIT future");
                    let (block, new_leaf, state) = x?;
                    let new_leaf_hash = new_leaf.hash();
                    let pl = phaselock.clone();
                    let _ctx = context.clone();
                    let fut = async move {
                        trace!("Waiting for threshold of precommit votes to arrive");
                        let mut vote_queue = pl
                            .inner
                            .precommit_vote_queue
                            .wait_for(|x| x.current_view == current_view)
                            .await;
                        debug!("Threshold number of precommit votes recieved");
                        let votes: Vec<_> = vote_queue
                            .drain(..)
                            .filter(|x| x.leaf_hash == new_leaf_hash)
                            .map(|x| (x.id, x.signature))
                            .collect();
                        // Generate a QC
                        let signature = generate_qc(
                            votes.iter().map(|(x, y)| (*x, y)),
                            &pl.inner.public_key.set,
                        )
                        .map_err(|e| {
                            PhaseLockError::FailedToAssembleQC {
                                stage: Stage::PreCommit,
                                source: e,
                            }
                        })?;
                        let qc = QuorumCertificate {
                            block_hash: BlockContents::hash(&block),
                            leaf_hash: new_leaf_hash,
                            view_number: current_view,
                            stage: Stage::PreCommit,
                            signature: Some(signature),
                            genesis: false,
                        };
                        debug!(?qc, "commit qc generated");
                        let pc_message = Message::Commit(Commit {
                            leaf_hash: new_leaf_hash,
                            qc,
                            current_view,
                        });
                        let network_result = pl
                            .inner
                            .networking
                            .broadcast_message(pc_message)
                            .await
                            .context(FailedToBroadcastSnafu {
                                stage: Stage::Commit,
                            });
                        if let Err(e) = network_result {
                            warn!(?e, "Failed to broadcast commit message");
                        }
                        debug!("Commit message sent");
                        // Make a pre commit vote and send it to ourselves
                        let signature = pl.inner.private_key.partial_sign(
                            &new_leaf_hash,
                            Stage::Commit,
                            current_view,
                        );
                        let vote_message = Vote {
                            leaf_hash: new_leaf_hash,
                            signature,
                            id: pl.inner.public_key.nonce,
                            current_view,
                            stage: Stage::Commit,
                        };
                        pl.inner.commit_vote_queue.push(vote_message).await;

                        Ok((block, new_leaf, state))
                    }
                    .boxed();
                    *self = SequentialLeader::Commit(DebugFuture(fut));
                    Poll::Ready(Err(PhaseLockError::Continue))
                } else {
                    Poll::Pending
                }
            }
            // Evaluate the COMMIT future and setup the DECIDE future
            SequentialLeader::Commit(fut) => {
                if let Poll::Ready(x) = Future::poll(fut.0.as_mut(), cx) {
                    debug!(?x, "Results from the COMMIT future");
                    let (block, new_leaf, state) = x?;
                    let new_leaf_hash = new_leaf.hash();
                    let pl = phaselock.clone();
                    let ctx = context.clone();
                    let fut = async move {
                        let mut vote_queue = pl
                            .inner
                            .commit_vote_queue
                            .wait_for(|x| x.current_view == current_view)
                            .await;
                        let votes: Vec<_> = vote_queue
                            .drain(..)
                            .filter(|x| x.leaf_hash == new_leaf_hash)
                            .map(|x| (x.id, x.signature))
                            .collect();
                        // Generate QC
                        let signature = generate_qc(
                            votes.iter().map(|(x, y)| (*x, y)),
                            &pl.inner.public_key.set,
                        )
                        .map_err(|e| {
                            PhaseLockError::FailedToAssembleQC {
                                stage: Stage::Decide,
                                source: e,
                            }
                        })?;
                        let qc = QuorumCertificate {
                            block_hash: BlockContents::hash(&block),
                            leaf_hash: new_leaf_hash,
                            view_number: current_view,
                            stage: Stage::Commit,
                            signature: Some(signature),
                            genesis: false,
                        };
                        debug!(?qc, "decide qc generated");
                        // Find blocks and states that were commited
                        let mut old_state = pl.inner.committed_state.write().await;
                        let mut old_leaf = pl.inner.committed_leaf.write().await;
                        let mut events = vec![];
                        let mut walk_leaf = new_leaf_hash;
                        while walk_leaf != *old_leaf {
                            debug!(?walk_leaf, "Looping");
                            let block = match pl.inner.storage.get_leaf(&walk_leaf).await {
                                StorageResult::Some(x) => x,
                                StorageResult::None => {
                                    warn!(?walk_leaf, "Parent did not exist in store");
                                    break;
                                }
                                StorageResult::Err(e) => {
                                    warn!(?walk_leaf, ?e, "Error finding parent");
                                    break;
                                }
                            };
                            let state = match pl.inner.storage.get_state(&walk_leaf).await {
                                StorageResult::Some(x) => x,
                                StorageResult::None => {
                                    warn!(?walk_leaf, "Parent did not exist in store");
                                    break;
                                }
                                StorageResult::Err(e) => {
                                    warn!(?walk_leaf, ?e, "Error finding parent");
                                    break;
                                }
                            };
                            state.on_commit();
                            events.push((block.item, state));
                            walk_leaf = block.parent;
                        }
                        info!(?events, "Sending decide events");
                        // Send decide event
                        pl.inner.stateful_handler.lock().await.notify(
                            events.iter().map(|(x, _)| x.clone()).collect(),
                            events.iter().map(|(_, x)| x.clone()).collect(),
                        );
                        ctx.send_decide(current_view, &events);

                        // Add qc to decision cache
                        pl.inner.storage.insert_qc(qc.clone()).await;
                        *old_state = Arc::new(state);
                        *old_leaf = new_leaf_hash;
                        trace!("New state written");
                        // Broadcast the decision
                        let d_message = Message::Decide(Decide {
                            leaf_hash: new_leaf_hash,
                            qc,
                            current_view,
                        });

                        let network_result = pl
                            .inner
                            .networking
                            .broadcast_message(d_message)
                            .await
                            .context(FailedToBroadcastSnafu {
                                stage: Stage::Decide,
                            });

                        if let Err(e) = network_result {
                            warn!(?e, "Error broadcasting decision");
                        }
                        debug!("Decision broadcasted");

                        Ok(current_view)
                    }
                    .boxed();
                    *self = SequentialLeader::Decide(DebugFuture(fut));
                    Poll::Ready(Err(PhaseLockError::Continue))
                } else {
                    Poll::Pending
                }
            }
            // Evaluate the DECIDE future
            SequentialLeader::Decide(fut) => match Future::poll(fut.0.as_mut(), cx) {
                Poll::Ready(x) => {
                    debug!(?x, "Results from DECIDE future");
                    *self = SequentialLeader::End(x);
                    Poll::Ready(Err(PhaseLockError::Continue))
                }
                Poll::Pending => Poll::Pending,
            },
            SequentialLeader::End(x) => {
                let mut y = Ok(0);
                std::mem::swap(x, &mut y);
                Poll::Ready(y)
            }
        }
    }
}

/// Represents the replica logic for a round of Sequential [`PhaseLock`]
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum SequentialReplica<I: NodeImplementation<N> + 'static, const N: usize> {
    /// Initial starting state for a replica
    Start,
    /// Setup the PREPARE future
    BeforePrepare(Prepare<I::Block, I::State, N>),
    /// execute PREPARE phase
    PrepareStage(DebugFuture<'static, Result<Leaf<I::Block, N>>>),
    /// Setup the PRECOMMIT future
    BeforePreCommit {
        /// Leaf we are acting on
        leaf: Leaf<I::Block, N>,
        /// Message that triggered this action
        message: PreCommit<N>,
    },
    /// Execute the PRECOMMIT phase
    PreCommitStage(DebugFuture<'static, Result<Leaf<I::Block, N>>>),
    /// Setup the COMMIT future
    BeforeCommit {
        /// Leaf we are acting on
        leaf: Leaf<I::Block, N>,
        /// Message that triggered this action
        message: Commit<N>,
    },
    /// Execute the COMMIT stage
    CommitStage(DebugFuture<'static, Result<Leaf<I::Block, N>>>),
    /// Setup the DECIDE future
    BeforeDecide {
        /// Leaf we are acting on
        leaf: Leaf<I::Block, N>,
        /// Message that triggered this action        
        message: Decide<N>,
    },
    /// Execute the decide stage
    DecideStage(DebugFuture<'static, Result<u64>>),
    /// Setup the DECIDE future in the event of a sort circut
    BeforeDecideShort {
        /// Decide message we are shorting on
        message: Decide<N>,
    },
    /// Execute a short circut decide
    DecideShort(DebugFuture<'static, Result<u64>>),
    /// Wait for a message matching the current stage or later to come in
    WaitForMessage {
        /// Leaf to pass on
        leaf: Option<Leaf<I::Block, N>>,
        /// Stage we are waiting for
        stage: Stage,
    },
    /// Waiting for a message
    WaitingForMessage(DebugFuture<'static, Result<WaitResult<I::Block, I::State, N>>>),
    /// Round has finished or faulted
    End(Result<u64>),
}

// TODO: Short circuiting logic
// Probably do this by creating a wrapper method that checks the channels and then calls the future
// if no commit qc with a higher view_number was found
impl<I: NodeImplementation<N> + 'static + Send + Sync, const N: usize> SequentialReplica<I, N> {
    /// Polls the replica variant of the state machine
    #[instrument(skip(cx, phaselock), fields(id = phaselock.inner.public_key.nonce))]
    #[allow(clippy::enum_glob_use)]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        phaselock: &mut PhaseLock<I, N>,
        current_view: u64,
        leader: &PubKey,
        context: &Ctx<I, N>,
    ) -> Poll<Result<u64>> {
        use SequentialReplica::*;
        match self {
            End(x) => {
                // Steal the value and replace it.
                // This will break polling after we have already returned `Ready`, but that is ok
                let mut y = Ok(0);
                std::mem::swap(x, &mut y);
                Poll::Ready(y)
            }
            Start => {
                debug!("Setting up wait for prepare message");
                *self = WaitForMessage {
                    leaf: None,
                    stage: Stage::Prepare,
                };
                Poll::Ready(Err(PhaseLockError::Continue))
            }
            BeforePrepare(prepare) => {
                debug!("Preapre message recieved from leader!!!");
                let leaf = prepare.leaf.clone();
                let leaf_hash = leaf.hash();
                let pl = phaselock.clone();
                let high_qc = prepare.high_qc.clone();
                let ctx = context.clone();
                let fut = async move {
                    debug!(?leaf, "Looking for state for leaf");
                    let state = match pl.inner.storage.get_state(&leaf.parent).await {
                        StorageResult::Some(x) => Ok(x),
                        StorageResult::None => Err(PhaseLockError::ItemNotFound {
                            hash: leaf.parent.to_vec(),
                        }),
                        StorageResult::Err(e) => Err(PhaseLockError::StorageError { err: e }),
                    }?;

                    // Check that the message is safe, extends from the given qc, and is valid given
                    // the current state
                    let is_safe_node = pl.safe_node(&leaf, &high_qc).await;
                    if is_safe_node && state.validate_block(&leaf.item) {
                        let signature = pl.inner.private_key.partial_sign(
                            &leaf_hash,
                            Stage::Prepare,
                            current_view,
                        );
                        let vote = Vote {
                            signature,
                            id: pl.inner.public_key.nonce,
                            leaf_hash,
                            current_view,
                            stage: Stage::Prepare,
                        };
                        let vote_message = Message::PrepareVote(vote);
                        let network_result = pl
                            .inner
                            .networking
                            .message_node(vote_message, pl.inner.get_leader(current_view))
                            .await
                            .context(FailedToMessageLeaderSnafu {
                                stage: Stage::Prepare,
                            });
                        if let Err(e) = network_result {
                            warn!(?e, "Error submitting prepare vote");
                        } else {
                            debug!("Prepare message successfully processed");
                        }
                        ctx.send_propose(current_view, &leaf.item);
                        // Add resulting state to storage
                        let new_state = state.append(&leaf.item).map_err(|error| {
                            error!(?error, "Failed to append block to existing state");
                            PhaseLockError::InconsistentBlock {
                                stage: Stage::Prepare,
                            }
                        })?;
                        // Insert new state into storage
                        debug!(?new_state, "New state inserted");
                        match pl.inner.storage.insert_state(new_state, leaf_hash).await {
                            StorageResult::Some(_) => Ok(leaf),
                            StorageResult::None => Ok(leaf),
                            StorageResult::Err(e) => Err(PhaseLockError::StorageError { err: e }),
                        }
                    } else {
                        error!("is_safe_node: {}", is_safe_node);
                        error!(?leaf, "Leaf failed safe_node predicate");
                        Err(PhaseLockError::BadBlock {
                            stage: Stage::Prepare,
                        })
                    }
                }
                .boxed();
                *self = PrepareStage(DebugFuture(fut));
                Poll::Ready(Err(PhaseLockError::Continue))
            }
            PrepareStage(fut) => match Future::poll(fut.0.as_mut(), cx) {
                Poll::Ready(x) => {
                    debug!(?x, "Results from prepare future, setting up precommit");
                    let leaf = x?;
                    *self = WaitForMessage {
                        leaf: Some(leaf),
                        stage: Stage::PreCommit,
                    };
                    Poll::Ready(Err(PhaseLockError::Continue))
                }
                Poll::Pending => Poll::Pending,
            },
            BeforePreCommit { leaf, message } => {
                debug!("Precommit message recieved from leader!!!");
                let leaf = leaf.clone();
                let leaf_hash = leaf.hash();
                let message = message.clone();
                let pl = phaselock.clone();
                let fut = async move {
                    let prepare_qc = message.qc;
                    if !(prepare_qc.verify(&pl.inner.public_key.set, current_view, Stage::Prepare)
                        && prepare_qc.leaf_hash == leaf_hash)
                    {
                        error!(?prepare_qc, "Bad or forged prepare_qc");
                        return Err(PhaseLockError::BadOrForgedQC {
                            stage: Stage::PreCommit,
                            bad_qc: prepare_qc.to_vec_cert(),
                        });
                    }
                    debug!("Precommit qc validated");
                    let signature = pl.inner.private_key.partial_sign(
                        &leaf_hash,
                        Stage::PreCommit,
                        current_view,
                    );
                    let vote_message = Message::PreCommitVote(Vote {
                        leaf_hash,
                        signature,
                        id: pl.inner.public_key.nonce,
                        current_view,
                        stage: Stage::PreCommit,
                    });
                    // store the prepare qc
                    let mut pqc = pl.inner.prepare_qc.write().await;
                    *pqc = Some(prepare_qc);
                    trace!("Prepare qc stored");
                    // send the vote message
                    let network_result = pl
                        .inner
                        .networking
                        .message_node(vote_message, pl.inner.get_leader(current_view))
                        .await
                        .context(FailedToMessageLeaderSnafu {
                            stage: Stage::PreCommit,
                        });
                    if let Err(e) = network_result {
                        warn!(?e, "Error submitting the precommit vote");
                    } else {
                        debug!("Precommit vote sent");
                    }
                    Ok(leaf)
                }
                .boxed();
                *self = PreCommitStage(DebugFuture(fut));
                Poll::Ready(Err(PhaseLockError::Continue))
            }
            PreCommitStage(fut) => match Future::poll(fut.0.as_mut(), cx) {
                Poll::Ready(x) => {
                    debug!(?x, "Precommit stage results");
                    let leaf = x?;
                    *self = WaitForMessage {
                        leaf: Some(leaf),
                        stage: Stage::Commit,
                    };
                    Poll::Ready(Err(PhaseLockError::Continue))
                }
                Poll::Pending => Poll::Pending,
            },
            BeforeCommit { leaf, message } => {
                debug!("Commit message received from leader!!!");
                let leaf = leaf.clone();
                let leaf_hash = leaf.hash();
                let message = message.clone();
                let pl = phaselock.clone();
                let fut = async move {
                    // Verify QC
                    let pc_qc = message.qc.clone();
                    if !(pc_qc.verify(&pl.inner.public_key.set, current_view, Stage::PreCommit)
                        && pc_qc.leaf_hash == leaf_hash)
                    {
                        error!(?pc_qc, "Bad or forged precommit qc");
                        return Err(PhaseLockError::BadOrForgedQC {
                            stage: Stage::Commit,
                            bad_qc: pc_qc.to_vec_cert(),
                        });
                    }
                    // Update locked qc
                    let mut locked_qc = pl.inner.locked_qc.write().await;
                    *locked_qc = Some(pc_qc);
                    trace!("Locked qc updated");
                    let signature =
                        pl.inner
                            .private_key
                            .partial_sign(&leaf_hash, Stage::Commit, current_view);
                    let vote_message = Message::CommitVote(Vote {
                        leaf_hash,
                        signature,
                        id: pl.inner.public_key.nonce,
                        current_view,
                        stage: Stage::Commit,
                    });
                    trace!("Commit vote packed");
                    let network_result = pl
                        .inner
                        .networking
                        .message_node(vote_message, pl.inner.get_leader(current_view))
                        .await
                        .context(FailedToMessageLeaderSnafu {
                            stage: Stage::Commit,
                        });
                    if let Err(e) = network_result {
                        warn!(?e, "Error sending commit vote");
                    } else {
                        debug!("Commit vote sent to leader");
                    }
                    Ok(leaf)
                }
                .boxed();
                *self = CommitStage(DebugFuture(fut));
                Poll::Ready(Err(PhaseLockError::Continue))
            }
            CommitStage(fut) => match Future::poll(fut.0.as_mut(), cx) {
                Poll::Ready(x) => {
                    debug!(?x, "Commit stage results");
                    let leaf = x?;
                    *self = WaitForMessage {
                        leaf: Some(leaf),
                        stage: Stage::Decide,
                    };
                    Poll::Ready(Err(PhaseLockError::Continue))
                }
                Poll::Pending => Poll::Pending,
            },
            BeforeDecide { leaf, message } => {
                let leaf = leaf.clone();
                let leaf_hash = leaf.hash();
                let message = message.clone();
                let pl = phaselock.clone();
                let ctx = context.clone();
                let fut = async move {
                    let _ = &leaf;
                    let decide_qc = message.qc;
                    if !(decide_qc.verify(&pl.inner.public_key.set, current_view, Stage::Commit)
                        && decide_qc.leaf_hash == leaf_hash)
                    {
                        error!(?decide_qc, "Bad or forged commit qc");
                        return Err(PhaseLockError::BadOrForgedQC {
                            stage: Stage::Decide,
                            bad_qc: decide_qc.to_vec_cert(),
                        });
                    }
                    // apply the state
                    let state = match pl.inner.storage.get_state(&leaf_hash).await {
                        StorageResult::Some(x) => Ok(x),
                        StorageResult::None => Err(PhaseLockError::ItemNotFound {
                            hash: leaf.parent.to_vec(),
                        }),
                        StorageResult::Err(e) => Err(PhaseLockError::StorageError { err: e }),
                    }?;
                    let mut old_state = pl.inner.committed_state.write().await;
                    let mut old_leaf = pl.inner.committed_leaf.write().await;
                    let mut events = vec![];
                    let mut walk_leaf = leaf_hash;

                    while walk_leaf != *old_leaf {
                        debug!(?walk_leaf, "Looping");
                        let block = match pl.inner.storage.get_leaf(&walk_leaf).await {
                            StorageResult::Some(x) => x,
                            StorageResult::None => {
                                warn!(?walk_leaf, "Parent did not exist in store");
                                break;
                            }
                            StorageResult::Err(e) => {
                                warn!(?walk_leaf, ?e, "Error finding parent");
                                break;
                            }
                        };
                        let state = match pl.inner.storage.get_state(&walk_leaf).await {
                            StorageResult::Some(x) => x,
                            StorageResult::None => {
                                warn!(?walk_leaf, "Parent did not exist in store");
                                break;
                            }
                            StorageResult::Err(e) => {
                                warn!(?walk_leaf, ?e, "Error finding parent");
                                break;
                            }
                        };
                        state.on_commit();
                        events.push((block.item, state));
                        walk_leaf = block.parent;
                    }

                    info!(?events, "Sending decide events");
                    // Send decide event
                    pl.inner.stateful_handler.lock().await.notify(
                        events.iter().map(|(x, _)| x.clone()).collect(),
                        events.iter().map(|(_, x)| x.clone()).collect(),
                    );
                    ctx.send_decide(current_view, &events);
                    *old_state = Arc::new(state);
                    *old_leaf = leaf_hash;
                    debug!("Round finished");
                    Ok(current_view)
                }
                .boxed();
                *self = DecideStage(DebugFuture(fut));
                Poll::Ready(Err(PhaseLockError::Continue))
            }
            DecideStage(fut) => match Future::poll(fut.0.as_mut(), cx) {
                Poll::Ready(x) => {
                    *self = End(x);
                    Poll::Ready(Err(PhaseLockError::Continue))
                }
                Poll::Pending => Poll::Pending,
            },
            BeforeDecideShort { message } => {
                let message = message.clone();
                let pl = phaselock.clone();
                let ctx = context.clone();
                let fut = async move {
                    let decide_qc = message.qc;
                    if !(decide_qc.verify(
                        &pl.inner.public_key.set,
                        decide_qc.view_number,
                        Stage::Commit,
                    )) {
                        error!(?decide_qc, "Bad or forged commit qc");
                        return Err(PhaseLockError::BadOrForgedQC {
                            stage: Stage::Decide,
                            bad_qc: decide_qc.to_vec_cert(),
                        });
                    }
                    let leaf_hash = message.leaf_hash;
                    // apply the state
                    let state = match pl.inner.storage.get_state(&leaf_hash).await {
                        StorageResult::Some(x) => Ok(x),
                        StorageResult::None => Err(PhaseLockError::ItemNotFound {
                            hash: leaf_hash.to_vec(),
                        }),
                        StorageResult::Err(e) => Err(PhaseLockError::StorageError { err: e }),
                    }?;
                    let mut old_state = pl.inner.committed_state.write().await;
                    let mut old_leaf = pl.inner.committed_leaf.write().await;
                    let mut events = vec![];
                    let mut walk_leaf = leaf_hash;

                    while walk_leaf != *old_leaf {
                        debug!(?walk_leaf, "Looping");
                        let block = match pl.inner.storage.get_leaf(&walk_leaf).await {
                            StorageResult::Some(x) => x,
                            StorageResult::None => {
                                warn!(?walk_leaf, "Parent did not exist in store");
                                break;
                            }
                            StorageResult::Err(e) => {
                                warn!(?walk_leaf, ?e, "Error finding parent");
                                break;
                            }
                        };
                        let state = match pl.inner.storage.get_state(&walk_leaf).await {
                            StorageResult::Some(x) => x,
                            StorageResult::None => {
                                warn!(?walk_leaf, "Parent did not exist in store");
                                break;
                            }
                            StorageResult::Err(e) => {
                                warn!(?walk_leaf, ?e, "Error finding parent");
                                break;
                            }
                        };
                        state.on_commit();
                        events.push((block.item, state));
                        walk_leaf = block.parent;
                    }
                    info!(?events, "Sending decide events");
                    // Send decide event
                    pl.inner.stateful_handler.lock().await.notify(
                        events.iter().map(|(x, _)| x.clone()).collect(),
                        events.iter().map(|(_, x)| x.clone()).collect(),
                    );
                    ctx.send_decide(current_view, &events);
                    let mut pqc = pl.inner.prepare_qc.write().await;
                    let mut lqc = pl.inner.locked_qc.write().await;
                    *old_state = Arc::new(state);
                    *old_leaf = leaf_hash;
                    *pqc = Some(decide_qc.clone());
                    *lqc = Some(decide_qc.clone());
                    debug!("Round finished");
                    Ok(decide_qc.view_number)
                }
                .boxed();
                *self = DecideShort(DebugFuture(fut));
                Poll::Ready(Err(PhaseLockError::Continue))
            }
            DecideShort(fut) => match Future::poll(fut.0.as_mut(), cx) {
                Poll::Ready(x) => {
                    *self = End(x);
                    Poll::Ready(Err(PhaseLockError::Continue))
                }
                Poll::Pending => Poll::Pending,
            },
            WaitForMessage { leaf, stage } => {
                let pl = phaselock.clone();
                let leaf = leaf.take();
                let stage = *stage;
                let fut = async move {
                    // Wait for all messages and short circuit if needed
                    let mut prepare_fut = pl
                        .inner
                        .prepare_waiter
                        .wait_for(|x| x.current_view == current_view)
                        .boxed()
                        .fuse();
                    let mut precommit_fut = pl
                        .inner
                        .precommit_waiter
                        .wait_for(|x| x.current_view == current_view)
                        .boxed()
                        .fuse();
                    let mut commit_fut = pl
                        .inner
                        .commit_waiter
                        .wait_for(|x| x.current_view == current_view)
                        .boxed()
                        .fuse();
                    let mut decide_fut = pl
                        .inner
                        .decide_waiter
                        .wait_for(|x| x.current_view >= current_view)
                        .boxed()
                        .fuse();
                    match stage {
                        Stage::None => Err(PhaseLockError::InvalidState {
                            context: "Stage::None in WaitForMessage".to_string(),
                        }),
                        Stage::Prepare => {
                            debug!("Waiting for prepare");
                            futures::select_biased! {
                                x = decide_fut =>
                                    Ok(WaitResult::ShortCircuitDecide(x)),
                                x = prepare_fut => Ok(WaitResult::Prepare(x)),
                            }
                        }
                        Stage::PreCommit => {
                            debug!("Waiting for precommit");
                            let leaf = leaf.expect("No leaf in precommit");
                            futures::select_biased! {
                                x = decide_fut =>
                                    Ok(WaitResult::ShortCircuitDecide(x)),
                                x = precommit_fut => Ok(WaitResult::PreCommit(leaf, x)),
                            }
                        }
                        Stage::Commit => {
                            debug!("Waiting for commit");
                            let leaf = leaf.expect("No leaf in commit");
                            futures::select_biased! {
                                x = decide_fut =>
                                    Ok(WaitResult::ShortCircuitDecide(x)),
                                x = commit_fut => Ok(WaitResult::Commit(leaf, x)),
                            }
                        }
                        Stage::Decide => {
                            debug!("Waiting for decide");
                            let leaf = leaf.expect("No leaf in decide");
                            let x = decide_fut.await;
                            if x.current_view > current_view {
                                Ok(WaitResult::ShortCircuitDecide(x))
                            } else {
                                Ok(WaitResult::Decide(leaf, x))
                            }
                        }
                    }
                }
                .boxed();
                *self = WaitingForMessage(DebugFuture(fut));
                Poll::Ready(Err(PhaseLockError::Continue))
            }
            WaitingForMessage(fut) => match Future::poll(fut.0.as_mut(), cx) {
                Poll::Ready(x) => {
                    let x = x?;
                    match x {
                        WaitResult::Prepare(prepare) => *self = BeforePrepare(prepare),
                        WaitResult::PreCommit(leaf, message) => {
                            *self = BeforePreCommit { leaf, message }
                        }
                        WaitResult::Commit(leaf, message) => *self = BeforeCommit { leaf, message },
                        WaitResult::Decide(leaf, message) => *self = BeforeDecide { leaf, message },
                        WaitResult::ShortCircuitDecide(message) => {
                            *self = BeforeDecideShort { message }
                        }
                    }
                    Poll::Ready(Err(PhaseLockError::Continue))
                }
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

/// Return type for the message waiter
enum WaitResult<B, S, const N: usize> {
    /// Go to prepare stage
    Prepare(Prepare<B, S, N>),
    /// Goto precommit stage
    PreCommit(Leaf<B, N>, PreCommit<N>),
    /// Goto commit stage
    Commit(Leaf<B, N>, Commit<N>),
    /// Goto Decide stage
    Decide(Leaf<B, N>, Decide<N>),
    /// Goto Decide stage
    ShortCircuitDecide(Decide<N>),
}
