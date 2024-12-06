// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! Provides the core consensus types

use std::{
    collections::{BTreeMap, HashMap},
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use async_lock::{RwLock, RwLockReadGuard, RwLockUpgradableReadGuard, RwLockWriteGuard};
use committable::{Commitment, Committable};
use tracing::instrument;
use utils::anytrace::*;
use vec1::Vec1;

pub use crate::utils::{View, ViewInner};
use crate::{
    data::{Leaf2, QuorumProposal2, VidDisperse, VidDisperseShare},
    error::HotShotError,
    event::{HotShotAction, LeafInfo},
    message::Proposal,
    simple_certificate::{DaCertificate2, QuorumCertificate2},
    traits::{
        block_contents::BuilderFee,
        metrics::{Counter, Gauge, Histogram, Metrics, NoMetrics},
        node_implementation::{ConsensusTime, NodeType},
        signature_key::SignatureKey,
        BlockPayload, ValidatedState,
    },
    utils::{
        epoch_from_block_number, BuilderCommitment, LeafCommitment, StateAndDelta, Terminator,
    },
    vid::VidCommitment,
    vote::{Certificate, HasViewNumber},
};

/// A type alias for `HashMap<Commitment<T>, T>`
pub type CommitmentMap<T> = HashMap<Commitment<T>, T>;

/// A type alias for `BTreeMap<T::Time, HashMap<T::SignatureKey, Proposal<T, VidDisperseShare<T>>>>`
pub type VidShares<TYPES> = BTreeMap<
    <TYPES as NodeType>::View,
    HashMap<<TYPES as NodeType>::SignatureKey, Proposal<TYPES, VidDisperseShare<TYPES>>>,
>;

/// Type alias for consensus state wrapped in a lock.
pub type LockedConsensusState<TYPES> = Arc<RwLock<Consensus<TYPES>>>;

/// A thin wrapper around `LockedConsensusState` that helps debugging locks
#[derive(Clone, Debug)]
pub struct OuterConsensus<TYPES: NodeType> {
    /// Inner `LockedConsensusState`
    pub inner_consensus: LockedConsensusState<TYPES>,
}

impl<TYPES: NodeType> OuterConsensus<TYPES> {
    /// Create a new instance of `OuterConsensus`, hopefully uniquely named
    pub fn new(consensus: LockedConsensusState<TYPES>) -> Self {
        Self {
            inner_consensus: consensus,
        }
    }

    /// Locks inner consensus for reading and leaves debug traces
    #[instrument(skip_all, target = "OuterConsensus")]
    pub async fn read(&self) -> ConsensusReadLockGuard<'_, TYPES> {
        tracing::trace!("Trying to acquire read lock on consensus");
        let ret = self.inner_consensus.read().await;
        tracing::trace!("Acquired read lock on consensus");
        ConsensusReadLockGuard::new(ret)
    }

    /// Locks inner consensus for writing and leaves debug traces
    #[instrument(skip_all, target = "OuterConsensus")]
    pub async fn write(&self) -> ConsensusWriteLockGuard<'_, TYPES> {
        tracing::trace!("Trying to acquire write lock on consensus");
        let ret = self.inner_consensus.write().await;
        tracing::trace!("Acquired write lock on consensus");
        ConsensusWriteLockGuard::new(ret)
    }

    /// Tries to acquire write lock on inner consensus and leaves debug traces
    #[instrument(skip_all, target = "OuterConsensus")]
    pub fn try_write(&self) -> Option<ConsensusWriteLockGuard<'_, TYPES>> {
        tracing::trace!("Trying to acquire write lock on consensus");
        let ret = self.inner_consensus.try_write();
        if let Some(guard) = ret {
            tracing::trace!("Acquired write lock on consensus");
            Some(ConsensusWriteLockGuard::new(guard))
        } else {
            tracing::trace!("Failed to acquire write lock");
            None
        }
    }

    /// Acquires upgradable read lock on inner consensus and leaves debug traces
    #[instrument(skip_all, target = "OuterConsensus")]
    pub async fn upgradable_read(&self) -> ConsensusUpgradableReadLockGuard<'_, TYPES> {
        tracing::trace!("Trying to acquire upgradable read lock on consensus");
        let ret = self.inner_consensus.upgradable_read().await;
        tracing::trace!("Acquired upgradable read lock on consensus");
        ConsensusUpgradableReadLockGuard::new(ret)
    }

    /// Tries to acquire read lock on inner consensus and leaves debug traces
    #[instrument(skip_all, target = "OuterConsensus")]
    pub fn try_read(&self) -> Option<ConsensusReadLockGuard<'_, TYPES>> {
        tracing::trace!("Trying to acquire read lock on consensus");
        let ret = self.inner_consensus.try_read();
        if let Some(guard) = ret {
            tracing::trace!("Acquired read lock on consensus");
            Some(ConsensusReadLockGuard::new(guard))
        } else {
            tracing::trace!("Failed to acquire read lock");
            None
        }
    }
}

/// A thin wrapper around `RwLockReadGuard` for `Consensus` that leaves debug traces when the lock is freed
pub struct ConsensusReadLockGuard<'a, TYPES: NodeType> {
    /// Inner `RwLockReadGuard`
    lock_guard: RwLockReadGuard<'a, Consensus<TYPES>>,
}

impl<'a, TYPES: NodeType> ConsensusReadLockGuard<'a, TYPES> {
    /// Creates a new instance of `ConsensusReadLockGuard` with the same name as parent `OuterConsensus`
    #[must_use]
    pub fn new(lock_guard: RwLockReadGuard<'a, Consensus<TYPES>>) -> Self {
        Self { lock_guard }
    }
}

impl<'a, TYPES: NodeType> Deref for ConsensusReadLockGuard<'a, TYPES> {
    type Target = Consensus<TYPES>;
    fn deref(&self) -> &Self::Target {
        &self.lock_guard
    }
}

impl<'a, TYPES: NodeType> Drop for ConsensusReadLockGuard<'a, TYPES> {
    #[instrument(skip_all, target = "ConsensusReadLockGuard")]
    fn drop(&mut self) {
        tracing::trace!("Read lock on consensus dropped");
    }
}

/// A thin wrapper around `RwLockWriteGuard` for `Consensus` that leaves debug traces when the lock is freed
pub struct ConsensusWriteLockGuard<'a, TYPES: NodeType> {
    /// Inner `RwLockWriteGuard`
    lock_guard: RwLockWriteGuard<'a, Consensus<TYPES>>,
}

impl<'a, TYPES: NodeType> ConsensusWriteLockGuard<'a, TYPES> {
    /// Creates a new instance of `ConsensusWriteLockGuard` with the same name as parent `OuterConsensus`
    #[must_use]
    pub fn new(lock_guard: RwLockWriteGuard<'a, Consensus<TYPES>>) -> Self {
        Self { lock_guard }
    }
}

impl<'a, TYPES: NodeType> Deref for ConsensusWriteLockGuard<'a, TYPES> {
    type Target = Consensus<TYPES>;
    fn deref(&self) -> &Self::Target {
        &self.lock_guard
    }
}

impl<'a, TYPES: NodeType> DerefMut for ConsensusWriteLockGuard<'a, TYPES> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.lock_guard
    }
}

impl<'a, TYPES: NodeType> Drop for ConsensusWriteLockGuard<'a, TYPES> {
    #[instrument(skip_all, target = "ConsensusWriteLockGuard")]
    fn drop(&mut self) {
        tracing::debug!("Write lock on consensus dropped");
    }
}

/// A thin wrapper around `RwLockUpgradableReadGuard` for `Consensus` that leaves debug traces when the lock is freed or upgraded
pub struct ConsensusUpgradableReadLockGuard<'a, TYPES: NodeType> {
    /// Inner `RwLockUpgradableReadGuard`
    lock_guard: ManuallyDrop<RwLockUpgradableReadGuard<'a, Consensus<TYPES>>>,
    /// A helper bool to indicate whether inner lock has been unsafely taken or not
    taken: bool,
}

impl<'a, TYPES: NodeType> ConsensusUpgradableReadLockGuard<'a, TYPES> {
    /// Creates a new instance of `ConsensusUpgradableReadLockGuard` with the same name as parent `OuterConsensus`
    #[must_use]
    pub fn new(lock_guard: RwLockUpgradableReadGuard<'a, Consensus<TYPES>>) -> Self {
        Self {
            lock_guard: ManuallyDrop::new(lock_guard),
            taken: false,
        }
    }

    /// Upgrades the inner `RwLockUpgradableReadGuard` and leaves debug traces
    #[instrument(skip_all, target = "ConsensusUpgradableReadLockGuard")]
    pub async fn upgrade(mut guard: Self) -> ConsensusWriteLockGuard<'a, TYPES> {
        let inner_guard = unsafe { ManuallyDrop::take(&mut guard.lock_guard) };
        guard.taken = true;
        tracing::debug!("Trying to upgrade upgradable read lock on consensus");
        let ret = RwLockUpgradableReadGuard::upgrade(inner_guard).await;
        tracing::debug!("Upgraded upgradable read lock on consensus");
        ConsensusWriteLockGuard::new(ret)
    }
}

impl<'a, TYPES: NodeType> Deref for ConsensusUpgradableReadLockGuard<'a, TYPES> {
    type Target = Consensus<TYPES>;

    fn deref(&self) -> &Self::Target {
        &self.lock_guard
    }
}

impl<'a, TYPES: NodeType> Drop for ConsensusUpgradableReadLockGuard<'a, TYPES> {
    #[instrument(skip_all, target = "ConsensusUpgradableReadLockGuard")]
    fn drop(&mut self) {
        if !self.taken {
            unsafe { ManuallyDrop::drop(&mut self.lock_guard) }
            tracing::debug!("Upgradable read lock on consensus dropped");
        }
    }
}

/// A bundle of views that we have most recently performed some action
#[derive(Debug, Clone, Copy)]
struct HotShotActionViews<T: ConsensusTime> {
    /// View we last proposed in to the Quorum
    proposed: T,
    /// View we last voted in for a QuorumProposal
    voted: T,
    /// View we last proposed to the DA committee
    da_proposed: T,
    /// View we lasted voted for DA proposal
    da_vote: T,
}

impl<T: ConsensusTime> Default for HotShotActionViews<T> {
    fn default() -> Self {
        let genesis = T::genesis();
        Self {
            proposed: genesis,
            voted: genesis,
            da_proposed: genesis,
            da_vote: genesis,
        }
    }
}
impl<T: ConsensusTime> HotShotActionViews<T> {
    /// Create HotShotActionViews from a view number
    fn from_view(view: T) -> Self {
        Self {
            proposed: view,
            voted: view,
            da_proposed: view,
            da_vote: view,
        }
    }
}
/// A reference to the consensus algorithm
///
/// This will contain the state of all rounds.
#[derive(derive_more::Debug, Clone)]
pub struct Consensus<TYPES: NodeType> {
    /// The validated states that are currently loaded in memory.
    validated_state_map: BTreeMap<TYPES::View, View<TYPES>>,

    /// All the VID shares we've received for current and future views.
    vid_shares: VidShares<TYPES>,

    /// All the DA certs we've received for current and future views.
    /// view -> DA cert
    saved_da_certs: HashMap<TYPES::View, DaCertificate2<TYPES>>,

    /// View number that is currently on.
    cur_view: TYPES::View,

    /// Epoch number that is currently on.
    cur_epoch: TYPES::Epoch,

    /// Last proposals we sent out, None if we haven't proposed yet.
    /// Prevents duplicate proposals, and can be served to those trying to catchup
    last_proposals: BTreeMap<TYPES::View, Proposal<TYPES, QuorumProposal2<TYPES>>>,

    /// last view had a successful decide event
    last_decided_view: TYPES::View,

    /// The `locked_qc` view number
    locked_view: TYPES::View,

    /// Map of leaf hash -> leaf
    /// - contains undecided leaves
    /// - includes the MOST RECENT decided leaf
    saved_leaves: CommitmentMap<Leaf2<TYPES>>,

    /// Bundle of views which we performed the most recent action
    /// visibible to the network.  Actions are votes and proposals
    /// for DA and Quorum
    last_actions: HotShotActionViews<TYPES::View>,

    /// Saved payloads.
    ///
    /// Encoded transactions for every view if we got a payload for that view.
    saved_payloads: BTreeMap<TYPES::View, Arc<[u8]>>,

    /// the highqc per spec
    high_qc: QuorumCertificate2<TYPES>,

    /// A reference to the metrics trait
    pub metrics: Arc<ConsensusMetricsValue>,

    /// Number of blocks in an epoch, zero means there are no epochs
    pub epoch_height: u64,
}

/// Contains several `ConsensusMetrics` that we're interested in from the consensus interfaces
#[derive(Clone, Debug)]
pub struct ConsensusMetricsValue {
    /// The number of last synced block height
    pub last_synced_block_height: Box<dyn Gauge>,
    /// The number of last decided view
    pub last_decided_view: Box<dyn Gauge>,
    /// The number of the last voted view
    pub last_voted_view: Box<dyn Gauge>,
    /// Number of timestamp for the last decided time
    pub last_decided_time: Box<dyn Gauge>,
    /// The current view
    pub current_view: Box<dyn Gauge>,
    /// Number of views that are in-flight since the last decided view
    pub number_of_views_since_last_decide: Box<dyn Gauge>,
    /// Number of views that are in-flight since the last anchor view
    pub number_of_views_per_decide_event: Box<dyn Histogram>,
    /// Duration of views as leader
    pub view_duration_as_leader: Box<dyn Histogram>,
    /// Number of invalid QCs we've seen since the last commit.
    pub invalid_qc: Box<dyn Gauge>,
    /// Number of outstanding transactions
    pub outstanding_transactions: Box<dyn Gauge>,
    /// Memory size in bytes of the serialized transactions still outstanding
    pub outstanding_transactions_memory_size: Box<dyn Gauge>,
    /// Number of views that timed out
    pub number_of_timeouts: Box<dyn Counter>,
    /// Number of views that timed out as leader
    pub number_of_timeouts_as_leader: Box<dyn Counter>,
    /// The number of empty blocks that have been proposed
    pub number_of_empty_blocks_proposed: Box<dyn Counter>,
    /// Number of events in the hotshot event queue
    pub internal_event_queue_len: Box<dyn Gauge>,
}

impl ConsensusMetricsValue {
    /// Create a new instance of this [`ConsensusMetricsValue`] struct, setting all the counters and gauges
    #[must_use]
    pub fn new(metrics: &dyn Metrics) -> Self {
        Self {
            last_synced_block_height: metrics
                .create_gauge(String::from("last_synced_block_height"), None),
            last_decided_view: metrics.create_gauge(String::from("last_decided_view"), None),
            last_voted_view: metrics.create_gauge(String::from("last_voted_view"), None),
            last_decided_time: metrics.create_gauge(String::from("last_decided_time"), None),
            current_view: metrics.create_gauge(String::from("current_view"), None),
            number_of_views_since_last_decide: metrics
                .create_gauge(String::from("number_of_views_since_last_decide"), None),
            number_of_views_per_decide_event: metrics
                .create_histogram(String::from("number_of_views_per_decide_event"), None),
            view_duration_as_leader: metrics
                .create_histogram(String::from("view_duration_as_leader"), None),
            invalid_qc: metrics.create_gauge(String::from("invalid_qc"), None),
            outstanding_transactions: metrics
                .create_gauge(String::from("outstanding_transactions"), None),
            outstanding_transactions_memory_size: metrics
                .create_gauge(String::from("outstanding_transactions_memory_size"), None),
            number_of_timeouts: metrics.create_counter(String::from("number_of_timeouts"), None),
            number_of_timeouts_as_leader: metrics
                .create_counter(String::from("number_of_timeouts_as_leader"), None),
            number_of_empty_blocks_proposed: metrics
                .create_counter(String::from("number_of_empty_blocks_proposed"), None),
            internal_event_queue_len: metrics
                .create_gauge(String::from("internal_event_queue_len"), None),
        }
    }
}

impl Default for ConsensusMetricsValue {
    fn default() -> Self {
        Self::new(&*NoMetrics::boxed())
    }
}

impl<TYPES: NodeType> Consensus<TYPES> {
    /// Constructor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        validated_state_map: BTreeMap<TYPES::View, View<TYPES>>,
        cur_view: TYPES::View,
        cur_epoch: TYPES::Epoch,
        locked_view: TYPES::View,
        last_decided_view: TYPES::View,
        last_actioned_view: TYPES::View,
        last_proposals: BTreeMap<TYPES::View, Proposal<TYPES, QuorumProposal2<TYPES>>>,
        saved_leaves: CommitmentMap<Leaf2<TYPES>>,
        saved_payloads: BTreeMap<TYPES::View, Arc<[u8]>>,
        high_qc: QuorumCertificate2<TYPES>,
        metrics: Arc<ConsensusMetricsValue>,
        epoch_height: u64,
    ) -> Self {
        Consensus {
            validated_state_map,
            vid_shares: BTreeMap::new(),
            saved_da_certs: HashMap::new(),
            cur_view,
            cur_epoch,
            last_decided_view,
            last_proposals,
            last_actions: HotShotActionViews::from_view(last_actioned_view),
            locked_view,
            saved_leaves,
            saved_payloads,
            high_qc,
            metrics,
            epoch_height,
        }
    }

    /// Get the current view.
    pub fn cur_view(&self) -> TYPES::View {
        self.cur_view
    }

    /// Get the current epoch.
    pub fn cur_epoch(&self) -> TYPES::Epoch {
        self.cur_epoch
    }

    /// Get the last decided view.
    pub fn last_decided_view(&self) -> TYPES::View {
        self.last_decided_view
    }

    /// Get the locked view.
    pub fn locked_view(&self) -> TYPES::View {
        self.locked_view
    }

    /// Get the high QC.
    pub fn high_qc(&self) -> &QuorumCertificate2<TYPES> {
        &self.high_qc
    }

    /// Get the validated state map.
    pub fn validated_state_map(&self) -> &BTreeMap<TYPES::View, View<TYPES>> {
        &self.validated_state_map
    }

    /// Get the saved leaves.
    pub fn saved_leaves(&self) -> &CommitmentMap<Leaf2<TYPES>> {
        &self.saved_leaves
    }

    /// Get the saved payloads.
    pub fn saved_payloads(&self) -> &BTreeMap<TYPES::View, Arc<[u8]>> {
        &self.saved_payloads
    }

    /// Get the vid shares.
    pub fn vid_shares(&self) -> &VidShares<TYPES> {
        &self.vid_shares
    }

    /// Get the saved DA certs.
    pub fn saved_da_certs(&self) -> &HashMap<TYPES::View, DaCertificate2<TYPES>> {
        &self.saved_da_certs
    }

    /// Get the map of our recent proposals
    pub fn last_proposals(
        &self,
    ) -> &BTreeMap<TYPES::View, Proposal<TYPES, QuorumProposal2<TYPES>>> {
        &self.last_proposals
    }

    /// Update the current view.
    /// # Errors
    /// Can return an error when the new view_number is not higher than the existing view number.
    pub fn update_view(&mut self, view_number: TYPES::View) -> Result<()> {
        ensure!(
            view_number > self.cur_view,
            debug!("New view isn't newer than the current view.")
        );
        self.cur_view = view_number;
        Ok(())
    }

    /// Get the parent Leaf Info from a given leaf and our public key.
    /// Returns None if we don't have the data in out state
    pub fn parent_leaf_info(
        &self,
        leaf: &Leaf2<TYPES>,
        public_key: &TYPES::SignatureKey,
    ) -> Option<LeafInfo<TYPES>> {
        let parent_view_number = leaf.justify_qc().view_number();
        let parent_leaf = self
            .saved_leaves
            .get(&leaf.justify_qc().data().leaf_commit)?;
        let parent_state_and_delta = self.state_and_delta(parent_view_number);
        let (Some(state), delta) = parent_state_and_delta else {
            return None;
        };
        let parent_vid = self
            .vid_shares()
            .get(&parent_view_number)?
            .get(public_key)
            .cloned()
            .map(|prop| prop.data);

        Some(LeafInfo {
            leaf: parent_leaf.clone(),
            state,
            delta,
            vid_share: parent_vid,
        })
    }

    /// Update the current epoch.
    /// # Errors
    /// Can return an error when the new epoch_number is not higher than the existing epoch number.
    pub fn update_epoch(&mut self, epoch_number: TYPES::Epoch) -> Result<()> {
        ensure!(
            epoch_number > self.cur_epoch,
            debug!("New epoch isn't newer than the current epoch.")
        );
        tracing::trace!("Updating epoch from {} to {}", self.cur_epoch, epoch_number);
        self.cur_epoch = epoch_number;
        Ok(())
    }

    /// Update the last actioned view internally for votes and proposals
    ///
    /// Returns true if the action is for a newer view than the last action of that type
    pub fn update_action(&mut self, action: HotShotAction, view: TYPES::View) -> bool {
        let old_view = match action {
            HotShotAction::Vote => &mut self.last_actions.voted,
            HotShotAction::Propose => &mut self.last_actions.proposed,
            HotShotAction::DaPropose => &mut self.last_actions.da_proposed,
            HotShotAction::DaVote => {
                if view > self.last_actions.da_vote {
                    self.last_actions.da_vote = view;
                }
                // TODO Add logic to prevent double voting.  For now the simple check if
                // the last voted view is less than the view we are trying to vote doesn't work
                // because the leader of view n + 1 may propose to the DA (and we would vote)
                // before the leader of view n.
                return true;
            }
            _ => return true,
        };
        if view > *old_view {
            *old_view = view;
            return true;
        }
        false
    }

    /// reset last actions to genesis so we can resend events in tests
    pub fn reset_actions(&mut self) {
        self.last_actions = HotShotActionViews::default();
    }

    /// Update the last proposal.
    ///
    /// # Errors
    /// Can return an error when the new view_number is not higher than the existing proposed view number.
    pub fn update_proposed_view(
        &mut self,
        proposal: Proposal<TYPES, QuorumProposal2<TYPES>>,
    ) -> Result<()> {
        ensure!(
            proposal.data.view_number()
                > self
                    .last_proposals
                    .last_key_value()
                    .map_or(TYPES::View::genesis(), |(k, _)| { *k }),
            debug!("New view isn't newer than the previously proposed view.")
        );
        self.last_proposals
            .insert(proposal.data.view_number(), proposal);
        Ok(())
    }

    /// Update the last decided view.
    ///
    /// # Errors
    /// Can return an error when the new view_number is not higher than the existing decided view number.
    pub fn update_last_decided_view(&mut self, view_number: TYPES::View) -> Result<()> {
        ensure!(
            view_number > self.last_decided_view,
            debug!("New view isn't newer than the previously decided view.")
        );
        self.last_decided_view = view_number;
        Ok(())
    }

    /// Update the locked view.
    ///
    /// # Errors
    /// Can return an error when the new view_number is not higher than the existing locked view number.
    pub fn update_locked_view(&mut self, view_number: TYPES::View) -> Result<()> {
        ensure!(
            view_number > self.locked_view,
            debug!("New view isn't newer than the previously locked view.")
        );
        self.locked_view = view_number;
        Ok(())
    }

    /// Update the validated state map with a new view_number/view combo.
    ///
    /// # Errors
    /// Can return an error when the new view contains less information than the existing view
    /// with the same view number.
    pub fn update_da_view(
        &mut self,
        view_number: TYPES::View,
        payload_commitment: VidCommitment,
    ) -> Result<()> {
        let view = View {
            view_inner: ViewInner::Da { payload_commitment },
        };
        self.update_validated_state_map(view_number, view)
    }

    /// Update the validated state map with a new view_number/view combo.
    ///
    /// # Errors
    /// Can return an error when the new view contains less information than the existing view
    /// with the same view number.
    pub fn update_leaf(
        &mut self,
        leaf: Leaf2<TYPES>,
        state: Arc<TYPES::ValidatedState>,
        delta: Option<Arc<<TYPES::ValidatedState as ValidatedState<TYPES>>::Delta>>,
    ) -> Result<()> {
        let view_number = leaf.view_number();
        let view = View {
            view_inner: ViewInner::Leaf {
                leaf: leaf.commit(),
                state,
                delta,
            },
        };
        self.update_validated_state_map(view_number, view)?;
        self.update_saved_leaves(leaf);
        Ok(())
    }

    /// Update the validated state map with a new view_number/view combo.
    ///
    /// # Errors
    /// Can return an error when the new view contains less information than the existing view
    /// with the same view number.
    fn update_validated_state_map(
        &mut self,
        view_number: TYPES::View,
        new_view: View<TYPES>,
    ) -> Result<()> {
        if let Some(existing_view) = self.validated_state_map().get(&view_number) {
            if let ViewInner::Leaf {
                delta: ref existing_delta,
                ..
            } = existing_view.view_inner
            {
                if let ViewInner::Leaf {
                    delta: ref new_delta,
                    ..
                } = new_view.view_inner
                {
                    ensure!(
                         new_delta.is_some() || existing_delta.is_none(),
                         debug!("Skipping the state update to not override a `Leaf` view with `Some` state delta.")
                     );
                } else {
                    bail!("Skipping the state update to not override a `Leaf` view with a non-`Leaf` view.");
                }
            }
        }
        self.validated_state_map.insert(view_number, new_view);
        Ok(())
    }

    /// Update the saved leaves with a new leaf.
    fn update_saved_leaves(&mut self, leaf: Leaf2<TYPES>) {
        self.saved_leaves.insert(leaf.commit(), leaf);
    }

    /// Update the saved payloads with a new encoded transaction.
    ///
    /// # Errors
    /// Can return an error when there's an existing payload corresponding to the same view number.
    pub fn update_saved_payloads(
        &mut self,
        view_number: TYPES::View,
        encoded_transaction: Arc<[u8]>,
    ) -> Result<()> {
        ensure!(
            !self.saved_payloads.contains_key(&view_number),
            "Payload with the same view already exists."
        );
        self.saved_payloads.insert(view_number, encoded_transaction);
        Ok(())
    }

    /// Update the high QC if given a newer one.
    /// # Errors
    /// Can return an error when the provided high_qc is not newer than the existing entry.
    pub fn update_high_qc(&mut self, high_qc: QuorumCertificate2<TYPES>) -> Result<()> {
        ensure!(
            high_qc.view_number > self.high_qc.view_number || high_qc == self.high_qc,
            debug!("High QC with an equal or higher view exists.")
        );
        tracing::debug!("Updating high QC");
        self.high_qc = high_qc;

        Ok(())
    }

    /// Add a new entry to the vid_shares map.
    pub fn update_vid_shares(
        &mut self,
        view_number: TYPES::View,
        disperse: Proposal<TYPES, VidDisperseShare<TYPES>>,
    ) {
        self.vid_shares
            .entry(view_number)
            .or_default()
            .insert(disperse.data.recipient_key.clone(), disperse);
    }

    /// Add a new entry to the da_certs map.
    pub fn update_saved_da_certs(&mut self, view_number: TYPES::View, cert: DaCertificate2<TYPES>) {
        self.saved_da_certs.insert(view_number, cert);
    }

    /// gather information from the parent chain of leaves
    /// # Errors
    /// If the leaf or its ancestors are not found in storage
    pub fn visit_leaf_ancestors<F>(
        &self,
        start_from: TYPES::View,
        terminator: Terminator<TYPES::View>,
        ok_when_finished: bool,
        mut f: F,
    ) -> std::result::Result<(), HotShotError<TYPES>>
    where
        F: FnMut(
            &Leaf2<TYPES>,
            Arc<<TYPES as NodeType>::ValidatedState>,
            Option<Arc<<<TYPES as NodeType>::ValidatedState as ValidatedState<TYPES>>::Delta>>,
        ) -> bool,
    {
        let mut next_leaf = if let Some(view) = self.validated_state_map.get(&start_from) {
            view.leaf_commitment().ok_or_else(|| {
                HotShotError::InvalidState(format!(
                    "Visited failed view {start_from:?} leaf. Expected successful leaf"
                ))
            })?
        } else {
            return Err(HotShotError::InvalidState(format!(
                "View {start_from:?} leaf does not exist in state map "
            )));
        };

        while let Some(leaf) = self.saved_leaves.get(&next_leaf) {
            let view = leaf.view_number();
            if let (Some(state), delta) = self.state_and_delta(view) {
                if let Terminator::Exclusive(stop_before) = terminator {
                    if stop_before == view {
                        if ok_when_finished {
                            return Ok(());
                        }
                        break;
                    }
                }
                next_leaf = leaf.parent_commitment();
                if !f(leaf, state, delta) {
                    return Ok(());
                }
                if let Terminator::Inclusive(stop_after) = terminator {
                    if stop_after == view {
                        if ok_when_finished {
                            return Ok(());
                        }
                        break;
                    }
                }
            } else {
                return Err(HotShotError::InvalidState(format!(
                    "View {view:?} state does not exist in state map"
                )));
            }
        }
        Err(HotShotError::MissingLeaf(next_leaf))
    }

    /// Garbage collects based on state change right now, this removes from both the
    /// `saved_payloads` and `validated_state_map` fields of `Consensus`.
    /// # Panics
    /// On inconsistent stored entries
    pub fn collect_garbage(&mut self, old_anchor_view: TYPES::View, new_anchor_view: TYPES::View) {
        let gc_view = TYPES::View::new(new_anchor_view.saturating_sub(1));
        // state check
        let anchor_entry = self
            .validated_state_map
            .iter()
            .next()
            .expect("INCONSISTENT STATE: anchor leaf not in state map!");
        if **anchor_entry.0 != old_anchor_view.saturating_sub(1) {
            tracing::error!(
                "Something about GC has failed. Older leaf exists than the previous anchor leaf."
            );
        }
        // perform gc
        self.saved_da_certs
            .retain(|view_number, _| *view_number >= old_anchor_view);
        self.validated_state_map
            .range(old_anchor_view..gc_view)
            .filter_map(|(_view_number, view)| view.leaf_commitment())
            .for_each(|leaf| {
                self.saved_leaves.remove(&leaf);
            });
        self.validated_state_map = self.validated_state_map.split_off(&gc_view);
        self.saved_payloads = self.saved_payloads.split_off(&gc_view);
        self.vid_shares = self.vid_shares.split_off(&gc_view);
        self.last_proposals = self.last_proposals.split_off(&gc_view);
    }

    /// Gets the last decided leaf.
    ///
    /// # Panics
    /// if the last decided view's leaf does not exist in the state map or saved leaves, which
    /// should never happen.
    #[must_use]
    pub fn decided_leaf(&self) -> Leaf2<TYPES> {
        let decided_view_num = self.last_decided_view;
        let view = self.validated_state_map.get(&decided_view_num).unwrap();
        let leaf = view
            .leaf_commitment()
            .expect("Decided leaf not found! Consensus internally inconsistent");
        self.saved_leaves.get(&leaf).unwrap().clone()
    }

    /// Gets the validated state with the given view number, if in the state map.
    #[must_use]
    pub fn state(&self, view_number: TYPES::View) -> Option<&Arc<TYPES::ValidatedState>> {
        match self.validated_state_map.get(&view_number) {
            Some(view) => view.state(),
            None => None,
        }
    }

    /// Gets the validated state and state delta with the given view number, if in the state map.
    #[must_use]
    pub fn state_and_delta(&self, view_number: TYPES::View) -> StateAndDelta<TYPES> {
        match self.validated_state_map.get(&view_number) {
            Some(view) => view.state_and_delta(),
            None => (None, None),
        }
    }

    /// Gets the last decided validated state.
    ///
    /// # Panics
    /// If the last decided view's state does not exist in the state map, which should never
    /// happen.
    #[must_use]
    pub fn decided_state(&self) -> Arc<TYPES::ValidatedState> {
        let decided_view_num = self.last_decided_view;
        self.state_and_delta(decided_view_num)
            .0
            .expect("Decided state not found! Consensus internally inconsistent")
    }

    /// Associated helper function:
    /// Takes `LockedConsensusState` which will be updated; locks it for read and write accordingly.
    /// Calculates `VidDisperse` based on the view, the txns and the membership,
    /// and updates `vid_shares` map with the signed `VidDisperseShare` proposals.
    /// Returned `Option` indicates whether the update has actually happened or not.
    #[instrument(skip_all, target = "Consensus", fields(view = *view))]
    pub async fn calculate_and_update_vid(
        consensus: OuterConsensus<TYPES>,
        view: <TYPES as NodeType>::View,
        membership: Arc<TYPES::Membership>,
        private_key: &<TYPES::SignatureKey as SignatureKey>::PrivateKey,
        epoch: TYPES::Epoch,
    ) -> Option<()> {
        let txns = Arc::clone(consensus.read().await.saved_payloads().get(&view)?);
        let vid = VidDisperse::calculate_vid_disperse(txns, &membership, view, epoch, None).await;
        let shares = VidDisperseShare::from_vid_disperse(vid);
        let mut consensus_writer = consensus.write().await;
        for share in shares {
            if let Some(prop) = share.to_proposal(private_key) {
                consensus_writer.update_vid_shares(view, prop);
            }
        }
        Some(())
    }

    /// Return true if the QC takes part in forming an eQC, i.e.
    /// it is one of the 3-chain certificates but not the eQC itself
    pub fn is_qc_forming_eqc(&self, qc: &QuorumCertificate2<TYPES>) -> bool {
        let high_qc_leaf_commit = qc.data.leaf_commit;
        let is_high_qc_extended = self.is_leaf_extended(high_qc_leaf_commit);
        if is_high_qc_extended {
            tracing::debug!("We have formed an eQC!");
        }
        self.is_leaf_for_last_block(high_qc_leaf_commit) && !is_high_qc_extended
    }

    /// Returns true if our high qc is forming an eQC
    pub fn is_high_qc_forming_eqc(&self) -> bool {
        self.is_qc_forming_eqc(self.high_qc())
    }

    /// Return true if the given leaf takes part in forming an eQC, i.e.
    /// it is one of the 3-chain leaves but not the eQC leaf itself
    pub fn is_leaf_forming_eqc(&self, leaf_commit: LeafCommitment<TYPES>) -> bool {
        self.is_leaf_for_last_block(leaf_commit) && !self.is_leaf_extended(leaf_commit)
    }

    /// Returns true if the given leaf can form an extended Quorum Certificate
    /// The Extended Quorum Certificate (eQC) is the third Quorum Certificate formed in three
    /// consecutive views for the last block in the epoch.
    pub fn is_leaf_extended(&self, leaf_commit: LeafCommitment<TYPES>) -> bool {
        if !self.is_leaf_for_last_block(leaf_commit) {
            tracing::trace!("The given leaf is not for the last block in the epoch.");
            return false;
        }

        let Some(leaf) = self.saved_leaves.get(&leaf_commit) else {
            tracing::trace!("We don't have a leaf corresponding to the leaf commit");
            return false;
        };
        let leaf_view = leaf.view_number();
        let leaf_block_number = leaf.height();

        let mut last_visited_view_number = leaf_view;
        let mut is_leaf_extended = true;
        if let Err(e) = self.visit_leaf_ancestors(
            leaf_view,
            Terminator::Inclusive(leaf_view - 2),
            true,
            |leaf, _, _| {
                tracing::trace!(
                    "last_visited_view_number = {}, leaf.view_number = {}",
                    *last_visited_view_number,
                    *leaf.view_number()
                );

                if leaf.view_number() == leaf_view {
                    return true;
                }

                if last_visited_view_number - 1 != leaf.view_number() {
                    tracing::trace!("The chain is broken. Non consecutive views.");
                    is_leaf_extended = false;
                    return false;
                }
                if leaf_block_number != leaf.height() {
                    tracing::trace!("The chain is broken. Block numbers do not match.");
                    is_leaf_extended = false;
                    return false;
                }
                last_visited_view_number = leaf.view_number();
                true
            },
        ) {
            is_leaf_extended = false;
            tracing::debug!("Leaf ascension failed; error={e}");
        }
        tracing::trace!("Can the given leaf form an eQC? {}", is_leaf_extended);
        is_leaf_extended
    }

    /// Returns true if a given leaf is for the last block in the epoch
    pub fn is_leaf_for_last_block(&self, leaf_commit: LeafCommitment<TYPES>) -> bool {
        let Some(leaf) = self.saved_leaves.get(&leaf_commit) else {
            tracing::trace!("We don't have a leaf corresponding to the leaf commit");
            return false;
        };
        let block_height = leaf.height();
        if block_height == 0 || self.epoch_height == 0 {
            false
        } else {
            block_height % self.epoch_height == 0
        }
    }

    /// Returns true if the `parent_leaf` formed an eQC for the previous epoch to the `proposed_leaf`
    pub fn check_eqc(&self, proposed_leaf: &Leaf2<TYPES>, parent_leaf: &Leaf2<TYPES>) -> bool {
        if parent_leaf.view_number() == TYPES::View::genesis() {
            return true;
        }
        let new_epoch = epoch_from_block_number(proposed_leaf.height(), self.epoch_height);
        let old_epoch = epoch_from_block_number(parent_leaf.height(), self.epoch_height);

        new_epoch - 1 == old_epoch && self.is_leaf_extended(parent_leaf.commit())
    }
}

/// Alias for the block payload commitment and the associated metadata. The primary data
/// needed in order to submit a proposal.
#[derive(Eq, Hash, PartialEq, Debug, Clone)]
pub struct CommitmentAndMetadata<TYPES: NodeType> {
    /// Vid Commitment
    pub commitment: VidCommitment,
    /// Builder Commitment
    pub builder_commitment: BuilderCommitment,
    /// Metadata for the block payload
    pub metadata: <TYPES::BlockPayload as BlockPayload<TYPES>>::Metadata,
    /// Builder fee data
    pub fees: Vec1<BuilderFee<TYPES>>,
    /// View number this block is for
    pub block_view: TYPES::View,
    /// auction result that the block was produced from, if any
    pub auction_result: Option<TYPES::AuctionResult>,
}
