//! The consensus layer for hotshot. This currently implements the hotstuff paper: <https://arxiv.org/abs/1803.05069>
//!
//! To use this library, you should:
//! - Implement [`ConsensusApi`]
//! - Create a new instance of [`Consensus`]
//! - whenever a message arrives, call [`Consensus::add_consensus_message`]
//! - whenever a transaction arrives, call [`Consensus::add_transaction`]
//!

#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::panic
)]
#![allow(clippy::module_name_repetitions, clippy::unused_async)]

mod traits;

use commit::{Commitment, Committable};
pub use traits::ConsensusApi;

use async_std::sync::{Arc, RwLock, RwLockUpgradableReadGuard};
use flume::{Receiver, Sender};
use hotshot_types::{
    constants::GENESIS_VIEW,
    data::{Leaf, QuorumCertificate, ViewNumber},
    error::{HotShotError, RoundTimedoutState},
    event::TransactionCommitment,
    message::{ConsensusMessage, Proposal, TimedOut, Vote},
    traits::{
        block_contents::Genesis,
        node_implementation::{NodeImplementation, TypeMap},
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
        storage::{Storage, StoredView},
        BlockContents, StateContents,
    },
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::Deref,
};
use tracing::{error, info, instrument, warn};

/// A view's state
#[derive(Debug)]
pub enum ViewInner<STATE: StateContents> {
    /// Undecided view
    Leaf {
        /// Proposed leaf
        leaf: Commitment<Leaf<STATE>>,
    },
    /// Leaf has failed
    Failed,
}

impl<STATE: StateContents> ViewInner<STATE> {
    /// return the underlying leaf hash if it exists
    #[must_use]
    // TODO rename to commitment
    pub fn get_leaf_commitment(&self) -> Option<&Commitment<Leaf<STATE>>> {
        if let Self::Leaf { leaf } = self {
            Some(leaf)
        } else {
            None
        }
    }
}

impl<STATE: StateContents> Deref for View<STATE> {
    type Target = ViewInner<STATE>;

    fn deref(&self) -> &Self::Target {
        &self.view_inner
    }
}

/// struct containing messages for a view to send to replica
#[derive(Clone)]
pub struct ViewQueue<I: NodeImplementation> {
    /// to send networking events to Replica
    pub sender_chan: Sender<ConsensusMessage<I::State>>,

    /// to recv networking events for Replica
    pub receiver_chan: Receiver<ConsensusMessage<I::State>>,
}

impl<I: NodeImplementation> Default for ViewQueue<I> {
    /// create new view queue
    fn default() -> Self {
        let (s, r) = flume::unbounded();
        ViewQueue {
            sender_chan: s,
            receiver_chan: r,
        }
    }
}

/// metadata for sending information to replica (and in the future, the leader)
pub struct SendToTasks<I: NodeImplementation> {
    /// the current view number
    /// this should always be in sync with `Consensus`
    pub cur_view: ViewNumber,

    /// a map from view number to ViewQueue
    /// one of (replica|next leader)'s' task for view i will be listening on the channel in here
    pub channel_map: BTreeMap<ViewNumber, ViewQueue<I>>,
}

impl<I: NodeImplementation> SendToTasks<I> {
    /// create new sendtosasks
    #[must_use]
    pub fn new(view_num: ViewNumber) -> Self {
        SendToTasks {
            cur_view: view_num,
            channel_map: BTreeMap::default(),
        }
    }
}

/// This exists so we can perform state transitions mutably
#[derive(Debug)]
pub struct View<STATE: StateContents> {
    /// The view data. Wrapped in a struct so we can mutate
    pub view_inner: ViewInner<STATE>,
}

/// The result used in this crate
pub type Result<T = ()> = std::result::Result<T, HotShotError>;

/// A reference to the consensus algorithm
///
/// This will contain the state of all rounds.
#[derive(Debug)]
pub struct Consensus<I: NodeImplementation> {
    /// The phases that are currently loaded in memory
    // TODO(https://github.com/EspressoSystems/hotshot/issues/153): Allow this to be loaded from `Storage`?
    pub state_map: BTreeMap<ViewNumber, View<I::State>>,

    /// cur_view from pseudocode
    pub cur_view: ViewNumber,

    /// last view had a successful decide event
    pub last_decided_view: ViewNumber,

    /// A list of undecided transactions
    pub transactions: TransactionStorage<I>,

    /// Map of leaf hash -> leaf
    /// - contains undecided leaves
    /// - includes the MOST RECENT decided leaf
    pub saved_leaves: HashMap<Commitment<Leaf<I::State>>, Leaf<I::State>>,

    /// The `locked_qc` view number
    pub locked_view: ViewNumber,

    /// the highqc per spec
    pub high_qc: QuorumCertificate<I::State>,
}

/// This view's replica
#[derive(Debug, Clone)]
pub struct Replica<A: ConsensusApi<I>, I: NodeImplementation> {
    /// id of node
    pub id: u64,
    /// Reference to consensus. Replica will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<I>>>,
    /// channel for accepting leader proposals and timeouts messages
    pub proposal_collection_chan: Receiver<ConsensusMessage<I::State>>,
    /// view number this view is executing in
    pub cur_view: ViewNumber,
    /// genericQC from the pseudocode
    pub high_qc: QuorumCertificate<I::State>,
    /// hotshot consensus api
    pub api: A,
}

impl<A: ConsensusApi<I>, I: NodeImplementation> Replica<A, I> {
    /// portion of the replica task that spins until a valid QC can be signed or
    /// timeout is hit.
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "Replica Task", level = "error")]
    async fn find_valid_msg<'a>(
        &self,
        view_leader_key: <I as NodeImplementation>::SignatureKey,
        consensus: RwLockUpgradableReadGuard<'a, Consensus<I>>,
    ) -> (
        RwLockUpgradableReadGuard<'a, Consensus<I>>,
        std::result::Result<Leaf<I::State>, ()>,
    ) {
        let leaf = loop {
            let msg = self.proposal_collection_chan.recv_async().await;
            info!("recv-ed message {:?}", msg.clone());
            if let Ok(msg) = msg {
                // stale/newer view messages should never reach this specific task's receive channel
                if msg.view_number() != self.cur_view {
                    continue;
                }
                match msg {
                    ConsensusMessage::Proposal(p) => {
                        if !view_leader_key.validate(&p.signature, p.leaf.commit().as_ref()) {
                            warn!(?p.signature, "Could not verify proposal.");
                            continue;
                        }
                        let justify_qc = p.leaf.justify_qc;
                        let parent = if let Some(parent) =
                            consensus.saved_leaves.get(&p.leaf.parent_commitment)
                        {
                            parent
                        } else {
                            warn!("Proposal's parent missing from storage");
                            continue;
                        };

                        // validate the QC's signatures
                        if !self.api.validate_qc(&justify_qc) {
                            warn!(
                                "Proposal failure at qc verification {:?} vs {:?}",
                                justify_qc.view_number, parent.view_number
                            );
                            continue;
                        }

                        // check that we can indeed create the state
                        let leaf = if let Ok(state) = parent.state.append(&p.leaf.deltas) {
                            Leaf::new(
                                state,
                                p.leaf.deltas,
                                p.leaf.parent_commitment,
                                justify_qc.clone(),
                                self.cur_view,
                            )
                        } else {
                            warn!("State of proposal didn't match parent + deltas");
                            continue;
                        };

                        // TODO change to locked_view + 2 after VRF integration
                        let liveness_check = justify_qc.view_number > consensus.locked_view;

                        // check if proposal extends from the locked leaf
                        let safety_check = consensus
                            .visit_leaf_ancestors(
                                parent.view_number,
                                Terminator::Inclusive(consensus.locked_view),
                                false,
                                |leaf| {
                                    // if leaf view no == locked view no then we're done, report success by
                                    // returning true
                                    leaf.view_number != consensus.locked_view
                                },
                            )
                            .is_ok();

                        // NOTE safenode check is here
                        // if we pass the safenode check, vote
                        if safety_check || liveness_check {
                            let leaf_commitment = leaf.commit();
                            let signature = self.api.sign_vote(&leaf_commitment, self.cur_view);

                            let vote = ConsensusMessage::<I::State>::Vote(Vote {
                                block_commitment:
                                    <<I::State as StateContents>::Block as Committable>::commit(
                                        &leaf.deltas,
                                    ),
                                justify_qc: leaf.justify_qc.clone(),
                                signature,
                                leaf_commitment,
                                current_view: self.cur_view,
                            });

                            // send out vote

                            let next_leader = self.api.get_leader(self.cur_view + 1).await;

                            info!("Sending vote to next leader {:?}", vote);

                            if self
                                .api
                                .send_direct_message(next_leader, vote)
                                .await
                                .is_err()
                            {
                                warn!("Failed to send vote to next leader");
                            };
                        }

                        // even if we fail to vote

                        break leaf;
                    }
                    ConsensusMessage::NextViewInterrupt(_view_number) => {
                        let next_leader = self.api.get_leader(self.cur_view + 1).await;

                        let timed_out_msg = ConsensusMessage::TimedOut(TimedOut {
                            current_view: self.cur_view,
                            justify_qc: self.high_qc.clone(),
                        });
                        warn!(
                            "Timed out! Sending timeout to next leader {:?}",
                            timed_out_msg
                        );

                        // send timedout message to the next leader
                        let _result = self
                            .api
                            .send_direct_message(next_leader, timed_out_msg)
                            .await;

                        // exits from entire function
                        self.api.send_replica_timeout(self.cur_view).await;

                        return (consensus, Err(()));
                    }
                    ConsensusMessage::Vote(_) | ConsensusMessage::TimedOut(_) => {
                        // should only be for leader, never replica
                        warn!("Replica receieved a vote or timed out message. This is not what the replica expects. Skipping.");
                        continue;
                    }
                }
            }
            // fall through logic if we did not received successfully from channel
            warn!("Replica did not received successfully from channel. Terminating Replica.");
            self.api.send_replica_timeout(self.cur_view).await;
            return (consensus, Err(()));
        };
        (consensus, Ok(leaf))
    }

    /// run one view of replica
    /// returns the `high_qc`
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Replica Task", level = "error")]
    pub async fn run_view(self) -> QuorumCertificate<I::State> {
        info!("Replica task started!");
        let consensus = self.consensus.upgradable_read().await;
        let view_leader_key = self.api.get_leader(self.cur_view).await;

        let (consensus, maybe_leaf) = self.find_valid_msg(view_leader_key, consensus).await;

        if maybe_leaf.is_err() {
            return self.high_qc;
        }

        let leaf = maybe_leaf.unwrap();
        let mut new_anchor_view = consensus.last_decided_view;
        let mut new_locked_view = consensus.locked_view;
        let mut last_view_number_visited = self.cur_view;
        let mut new_commit_reached: bool = false;
        let mut new_decide_reached = false;
        let mut blocks = Vec::new();
        let mut states = Vec::new();
        let mut included_txns = HashSet::new();
        let mut qcs = Vec::new();
        let mut rejects = Vec::new();
        let old_anchor_view = consensus.last_decided_view;
        let parent_view = leaf.justify_qc.view_number;
        if parent_view + 1 == self.cur_view {
            let mut current_chain_length = 1usize;
            let _outcome = consensus.visit_leaf_ancestors(
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
                        // collecting chain elements for the decide
                        blocks.push(leaf.deltas.clone());
                        states.push(leaf.state.clone());
                        qcs.push(leaf.justify_qc.clone());
                        rejects.push(Vec::new());
                        let txns = leaf.deltas.contained_transactions();
                        for txn in txns {
                            included_txns.insert(txn);
                        }
                    }
                    true
                },
            );
        }
        let high_qc = leaf.justify_qc.clone();

        let included_txns_set: HashSet<_> = if new_decide_reached {
            included_txns
        } else {
            HashSet::new()
        };

        // promote lock here
        let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;
        consensus.state_map.insert(
            self.cur_view,
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
        if new_decide_reached {
            {
                let mut txns = consensus.transactions.write().await;
                *txns = txns
                    .drain()
                    .filter(|(txn_hash, _txn)| !included_txns_set.contains(txn_hash))
                    .collect();
            }

            let decide_sent =
                self.api
                    .send_decide(consensus.last_decided_view, blocks, states, qcs, rejects);
            let old_anchor_view = consensus.last_decided_view;
            consensus
                .collect_garbage(old_anchor_view, new_anchor_view)
                .await;
            consensus.last_decided_view = new_anchor_view;

            // We're only storing the last QC. We could store more but we're realistically only going to retrieve the last one.
            let storage = self.api.storage();
            // TODO(https://github.com/EspressoSystems/HotShot/issues/411): store the rejected transactions in this view
            let view_to_insert = StoredView::from(leaf);
            if let Err(e) = storage.append_single_view(view_to_insert).await {
                error!("Could not insert new anchor into the storage API: {:?}", e);
            }
            if let Err(e) = storage.cleanup_storage_up_to_view(old_anchor_view).await {
                error!(
                    "Could not clean up storage to view {:?}: {:?}",
                    old_anchor_view, e
                );
            }
            if let Err(e) = storage.commit().await {
                error!("Could not commit storage: {:?}", e);
            }
            decide_sent.await;
        }
        high_qc
    }
}

/// This view's Leader
#[derive(Debug, Clone)]
pub struct Leader<A: ConsensusApi<I>, I: NodeImplementation> {
    /// id of node
    pub id: u64,
    /// Reference to consensus. Leader will require a read lock on this.
    pub consensus: Arc<RwLock<Consensus<I>>>,
    /// The `high_qc` per spec
    pub high_qc: QuorumCertificate<I::State>,
    /// The view number we're running on
    pub cur_view: ViewNumber,
    /// Lock over the transactions list
    pub transactions: TransactionStorage<I>,
    /// Limited access to the consensus protocol
    pub api: A,
}

impl<A: ConsensusApi<I>, I: NodeImplementation> Leader<A, I> {
    /// TODO have this consume self instead of taking a mutable reference. We never use self again.
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Leader Task", level = "error")]
    pub async fn run_view(self) -> QuorumCertificate<I::State> {
        info!("Leader task started!");
        let parent_view_number = self.high_qc.view_number;

        let consensus = self.consensus.read().await;

        let mut reached_decided = false;

        let parent_leaf = if let Some(parent_view) = consensus.state_map.get(&parent_view_number) {
            match &parent_view.view_inner {
                ViewInner::Leaf { leaf } => {
                    if let Some(leaf) = consensus.saved_leaves.get(leaf) {
                        if leaf.view_number == consensus.last_decided_view {
                            reached_decided = true;
                        }
                        leaf
                    } else {
                        warn!("Failed to find high QC parent.");
                        return self.high_qc;
                    }
                }
                // can happen if future api is whacked
                ViewInner::Failed => {
                    warn!("Parent of high QC points to a failed QC");
                    return self.high_qc;
                }
            }
        } else {
            warn!("Couldn't find high QC parent in state map.");
            return self.high_qc;
        };

        let original_parent_hash = parent_leaf.commit();
        let starting_state = &parent_leaf.state;

        let mut previous_used_txns_vec = parent_leaf.deltas.contained_transactions();

        let mut next_parent_hash = original_parent_hash;

        if !reached_decided {
            while let Some(next_parent_leaf) = consensus.saved_leaves.get(&next_parent_hash) {
                if next_parent_leaf.view_number <= consensus.last_decided_view {
                    break;
                }
                let next_parent_txns = next_parent_leaf.deltas.contained_transactions();
                for next_parent_txn in next_parent_txns {
                    previous_used_txns_vec.insert(next_parent_txn);
                }
                next_parent_hash = next_parent_leaf.parent_commitment;
            }
            // TODO do some sort of sanity check on the view number that it matches decided
        }

        let previous_used_txns = previous_used_txns_vec
            .into_iter()
            .collect::<HashSet<TransactionCommitment<I::State>>>();

        let txns = self.transactions.read().await;

        let unclaimed_txns: Vec<_> = txns
            .iter()
            .filter(|(txn_hash, _txn)| !previous_used_txns.contains(txn_hash))
            .collect();

        let mut block = starting_state.next_block();
        for (_txn_hash, txn) in &unclaimed_txns {
            let new_block_check = block.add_transaction_raw(txn);
            if let Ok(new_block) = new_block_check {
                if starting_state.validate_block(&new_block) {
                    block = new_block;
                }
            }
        }

        if let Ok(new_state) = starting_state.append(&block) {
            let leaf = Leaf {
                view_number: self.cur_view,
                justify_qc: self.high_qc.clone(),
                parent_commitment: original_parent_hash,
                deltas: block,
                state: new_state,
            };
            let signature = self.api.sign_proposal(&leaf.commit(), self.cur_view);
            let message = ConsensusMessage::Proposal(Proposal { leaf, signature });
            info!("Sending out proposal {:?}", message);
            if self.api.send_broadcast_message(message).await.is_err() {
                warn!("Failed to broadcast to network");
            };
        } else {
            error!("Could not append state in high qc for proposal. Failed to send out proposal.");
        }

        self.high_qc.clone()
    }
}

/// The next view's leader
#[derive(Debug, Clone)]
pub struct NextLeader<A: ConsensusApi<I>, I: NodeImplementation> {
    /// id of node
    pub id: u64,
    /// generic_qc before starting this
    pub generic_qc: QuorumCertificate<I::State>,
    /// channel through which the leader collects votes
    pub vote_collection_chan: Receiver<ConsensusMessage<I::State>>,
    /// The view number we're running on
    pub cur_view: ViewNumber,
    /// Limited access to the consensus protocol
    pub api: A,
}

/// type alias for a less ugly mapping of signatures
pub type Signatures = BTreeMap<EncodedPublicKey, EncodedSignature>;

impl<A: ConsensusApi<I>, I: NodeImplementation> NextLeader<A, I> {
    /// Run one view of the next leader task
    /// # Panics
    /// While we are unwrapping, this function can logically never panic
    /// unless there is a bug in std
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Next Leader Task", level = "error")]
    pub async fn run_view(self) -> QuorumCertificate<I::State> {
        info!("Next Leader task started!");
        let mut qcs = HashSet::<QuorumCertificate<I::State>>::new();
        qcs.insert(self.generic_qc.clone());

        #[allow(clippy::type_complexity)]
        let mut vote_outcomes: HashMap<
            Commitment<Leaf<I::State>>,
            (Commitment<<I::State as StateContents>::Block>, Signatures),
        > = HashMap::new();
        // NOTE will need to refactor this during VRF integration
        let threshold = self.api.threshold();

        while let Ok(msg) = self.vote_collection_chan.recv_async().await {
            info!("recv-ed message {:?}", msg.clone());
            if msg.view_number() != self.cur_view {
                continue;
            }
            match msg {
                ConsensusMessage::TimedOut(t) => {
                    qcs.insert(t.justify_qc);
                }
                ConsensusMessage::Vote(vote) => {
                    // if the signature on the vote is invalid,
                    // assume it's sent by byzantine node
                    // and ignore
                    if !self.api.is_valid_signature(
                        &vote.signature.0,
                        &vote.signature.1,
                        vote.leaf_commitment,
                    ) {
                        continue;
                    }

                    qcs.insert(vote.justify_qc);

                    match vote_outcomes.entry(vote.leaf_commitment) {
                        std::collections::hash_map::Entry::Occupied(mut o) => {
                            let (bh, map) = o.get_mut();
                            if *bh != vote.block_commitment {
                                warn!("Mismatch between commitments in received votes. This is probably an error without byzantine nodes.");
                            }
                            map.insert(vote.signature.0.clone(), vote.signature.1.clone());
                        }
                        std::collections::hash_map::Entry::Vacant(location) => {
                            let mut map = BTreeMap::new();
                            map.insert(vote.signature.0, vote.signature.1);
                            location.insert((vote.block_commitment, map));
                        }
                    }

                    // unwraps here are fine since we *just* inserted the key
                    let (_, valid_signatures) = vote_outcomes.get(&vote.leaf_commitment).unwrap();

                    if valid_signatures.len() >= threshold.into() {
                        let (block_commitment, valid_signatures) =
                            vote_outcomes.remove(&vote.leaf_commitment).unwrap();
                        // construct QC
                        let qc = QuorumCertificate {
                            block_commitment,
                            leaf_commitment: vote.leaf_commitment,
                            view_number: self.cur_view,
                            signatures: valid_signatures,
                            genesis: false,
                        };
                        return qc;
                    }
                }
                ConsensusMessage::NextViewInterrupt(_view_number) => {
                    self.api.send_next_leader_timeout(self.cur_view).await;
                    break;
                }
                ConsensusMessage::Proposal(_p) => {
                    warn!("The next leader has received an unexpected proposal!");
                }
            }
        }

        qcs.into_iter().max_by_key(|qc| qc.view_number).unwrap()
    }
}

impl<I: NodeImplementation> Consensus<I> {
    /// increment the current view
    /// NOTE may need to do gc here
    pub fn increment_view(&mut self) -> ViewNumber {
        self.cur_view += 1;
        self.cur_view
    }

    /// gather information from the parent chain of leafs
    /// # Errors
    /// If the leaf or its ancestors are not found in storage
    pub fn visit_leaf_ancestors<F>(
        &self,
        start_from: ViewNumber,
        terminator: Terminator,
        ok_when_finished: bool,
        mut f: F,
    ) -> Result<()>
    where
        F: FnMut(&Leaf<I::State>) -> bool,
    {
        let mut next_leaf = if let Some(view) = self.state_map.get(&start_from) {
            *view
                .get_leaf_commitment()
                .ok_or_else(|| HotShotError::InvalidState {
                    context: format!(
                        "Visited failed view {:?} leaf. Expected successfuil leaf",
                        start_from
                    ),
                })?
        } else {
            return Err(HotShotError::InvalidState {
                context: format!("View {:?} leaf does not exist in state map ", start_from),
            });
        };

        while let Some(leaf) = self.saved_leaves.get(&next_leaf) {
            if let Terminator::Exclusive(stop_before) = terminator {
                if stop_before == leaf.view_number {
                    if ok_when_finished {
                        return Ok(());
                    }
                    break;
                }
            }
            next_leaf = leaf.parent_commitment;
            if !f(leaf) {
                return Ok(());
            }
            if let Terminator::Inclusive(stop_after) = terminator {
                if stop_after == leaf.view_number {
                    if ok_when_finished {
                        return Ok(());
                    }
                    break;
                }
            }
        }
        Err(HotShotError::LeafNotFound {})
    }

    /// garbage collects based on state change
    /// right now, this removes from both the `saved_leaves`
    /// and `state_map` fields of `Consensus`
    pub async fn collect_garbage(
        &mut self,
        old_anchor_view: ViewNumber,
        new_anchor_view: ViewNumber,
    ) {
        // state check
        let anchor_entry = self
            .state_map
            .iter()
            .next()
            .expect("INCONSISTENT STATE: anchor leaf not in state map!");
        if *anchor_entry.0 != old_anchor_view {
            error!(
                "Something about GC has failed. Older leaf exists than the previous anchor leaf."
            );
        }
        // perform gc
        self.state_map
            .range(old_anchor_view..new_anchor_view)
            .filter_map(|(_view_number, view)| view.get_leaf_commitment())
            .for_each(|leaf| {
                let _removed = self.saved_leaves.remove(leaf);
            });
        self.state_map = self.state_map.split_off(&new_anchor_view);
    }
}

/// A struct containing information about a finished round.
#[derive(Debug, Clone)]
pub struct RoundFinishedEvent {
    /// The round that finished
    pub view_number: ViewNumber,
    /// The state that this round finished as
    pub state: RoundFinishedEventState,
}

/// Contains the possible outcomes of a round.
///
/// The only successfully outcome is `Success`. More variants may be added to this enum but they will all be error states.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum RoundFinishedEventState {
    /// The round finished successfully
    Success,
    /// The round got interrupted
    Interrupted(RoundTimedoutState),
}

impl<I: NodeImplementation> Default for Consensus<I> {
    fn default() -> Self {
        Self {
            transactions: Arc::default(),
            cur_view: GENESIS_VIEW,
            last_decided_view: GENESIS_VIEW,
            state_map: BTreeMap::default(),
            saved_leaves: HashMap::default(),
            locked_view: GENESIS_VIEW,
            high_qc: <QuorumCertificate<I::State> as Genesis>::genesis(),
        }
    }
}

/// The type of a transaction
pub type TransactionStorage<I> =
Arc<
        RwLock<
            HashMap<
                Commitment<<<<I as NodeImplementation>::State as StateContents>::Block as BlockContents>::Transaction>,
                <I as TypeMap>::Transaction,
            >,
        >>;

impl<I: NodeImplementation> Consensus<I> {
    /// return a clone of the internal storage of unclaimed transactions
    #[must_use]
    pub fn get_transactions(&self) -> TransactionStorage<I> {
        self.transactions.clone()
    }

    /// Gets the last decided state
    /// # Panics
    /// if the last decided view's state does not exist in the state map
    /// this should never happen.
    #[must_use]
    pub fn get_decided_leaf(&self) -> Leaf<I::State> {
        let decided_view_num = self.last_decided_view;
        let view = self.state_map.get(&decided_view_num).unwrap();
        let leaf = view
            .get_leaf_commitment()
            .expect("Decided state not found! Consensus internally inconsistent");
        self.saved_leaves.get(leaf).unwrap().clone()
    }
}

/// Whether or not to stop inclusively or exclusively when walking
#[derive(Copy, Clone, Debug)]
pub enum Terminator {
    /// Stop right before this view number
    Exclusive(ViewNumber),
    /// Stop including this view number
    Inclusive(ViewNumber),
}
