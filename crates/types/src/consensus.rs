//! Provides the core consensus types

pub use crate::utils::{View, ViewInner};
use displaydoc::Display;

use crate::{
    data::Leaf,
    error::HotShotError,
    simple_certificate::{DACertificate, QuorumCertificate, UpgradeCertificate},
    traits::{
        metrics::{Counter, Gauge, Histogram, Label, Metrics, NoMetrics},
        node_implementation::NodeType,
        ValidatedState,
    },
    utils::{StateAndDelta, Terminator},
};
use commit::Commitment;

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
};
use tracing::error;

/// A type alias for `HashMap<Commitment<T>, T>`
type CommitmentMap<T> = HashMap<Commitment<T>, T>;

/// A reference to the consensus algorithm
///
/// This will contain the state of all rounds.
#[derive(custom_debug::Debug)]
pub struct Consensus<TYPES: NodeType> {
    /// Immutable instance-level state.
    pub instance_state: TYPES::InstanceState,

    /// The validated states that are currently loaded in memory.
    pub validated_state_map: BTreeMap<TYPES::Time, View<TYPES>>,

    /// All the DA certs we've received for current and future views.
    /// view -> DA cert
    pub saved_da_certs: HashMap<TYPES::Time, DACertificate<TYPES>>,

    /// All the upgrade certs we've received for current and future views.
    /// view -> upgrade cert
    pub saved_upgrade_certs: HashMap<TYPES::Time, UpgradeCertificate<TYPES>>,

    /// View number that is currently on.
    pub cur_view: TYPES::Time,

    /// last view had a successful decide event
    pub last_decided_view: TYPES::Time,

    /// Map of leaf hash -> leaf
    /// - contains undecided leaves
    /// - includes the MOST RECENT decided leaf
    pub saved_leaves: CommitmentMap<Leaf<TYPES>>,

    /// Saved payloads.
    ///
    /// Encoded transactions for every view if we got a payload for that view.
    pub saved_payloads: BTreeMap<TYPES::Time, Vec<u8>>,

    /// The `locked_qc` view number
    pub locked_view: TYPES::Time,

    /// the highqc per spec
    pub high_qc: QuorumCertificate<TYPES>,

    /// A reference to the metrics trait
    pub metrics: Arc<ConsensusMetricsValue>,
}

/// Contains several `ConsensusMetrics` that we're interested in from the consensus interfaces
#[derive(Clone, Debug)]
pub struct ConsensusMetricsValue {
    /// The number of last synced block height
    pub last_synced_block_height: Box<dyn Gauge>,
    /// The number of last decided view
    pub last_decided_view: Box<dyn Gauge>,
    /// Number of timestamp for the last decided time
    pub last_decided_time: Box<dyn Gauge>,
    /// The current view
    pub current_view: Box<dyn Gauge>,
    /// Number of views that are in-flight since the last decided view
    pub number_of_views_since_last_decide: Box<dyn Gauge>,
    /// Number of views that are in-flight since the last anchor view
    pub number_of_views_per_decide_event: Box<dyn Histogram>,
    /// Number of invalid QCs we've seen since the last commit.
    pub invalid_qc: Box<dyn Gauge>,
    /// Number of outstanding transactions
    pub outstanding_transactions: Box<dyn Gauge>,
    /// Memory size in bytes of the serialized transactions still outstanding
    pub outstanding_transactions_memory_size: Box<dyn Gauge>,
    /// Number of views that timed out
    pub number_of_timeouts: Box<dyn Counter>,
}

/// The wrapper with a string name for the networking metrics
#[derive(Clone, Debug)]
pub struct ConsensusMetrics {
    /// a prefix which tracks the name of the metric
    prefix: String,
    /// a map of values
    values: Arc<Mutex<InnerConsensusMetrics>>,
}

/// the set of counters and gauges for the networking metrics
#[derive(Clone, Debug, Default, Display)]
pub struct InnerConsensusMetrics {
    /// All the counters of the networking metrics
    pub counters: HashMap<String, usize>,
    /// All the gauges of the networking metrics
    pub gauges: HashMap<String, usize>,
    /// All the histograms of the networking metrics
    pub histograms: HashMap<String, Vec<f64>>,
    /// All the labels of the networking metrics
    pub labels: HashMap<String, String>,
}

impl ConsensusMetrics {
    #[must_use]
    /// For the creation and naming of gauge, counter, histogram and label.
    pub fn sub(&self, name: String) -> Self {
        let prefix = if self.prefix.is_empty() {
            name
        } else {
            format!("{}-{name}", self.prefix)
        };
        Self {
            prefix,
            values: Arc::clone(&self.values),
        }
    }
}

impl Metrics for ConsensusMetrics {
    fn create_counter(&self, label: String, _unit_label: Option<String>) -> Box<dyn Counter> {
        Box::new(self.sub(label))
    }

    fn create_gauge(&self, label: String, _unit_label: Option<String>) -> Box<dyn Gauge> {
        Box::new(self.sub(label))
    }

    fn create_histogram(&self, label: String, _unit_label: Option<String>) -> Box<dyn Histogram> {
        Box::new(self.sub(label))
    }

    fn create_label(&self, label: String) -> Box<dyn Label> {
        Box::new(self.sub(label))
    }

    fn subgroup(&self, subgroup_name: String) -> Box<dyn Metrics> {
        Box::new(self.sub(subgroup_name))
    }
}

impl Counter for ConsensusMetrics {
    fn add(&self, amount: usize) {
        *self
            .values
            .lock()
            .unwrap()
            .counters
            .entry(self.prefix.clone())
            .or_default() += amount;
    }
}

impl Gauge for ConsensusMetrics {
    fn set(&self, amount: usize) {
        *self
            .values
            .lock()
            .unwrap()
            .gauges
            .entry(self.prefix.clone())
            .or_default() = amount;
    }
    fn update(&self, delta: i64) {
        let mut values = self.values.lock().unwrap();
        let value = values.gauges.entry(self.prefix.clone()).or_default();
        let signed_value = i64::try_from(*value).unwrap_or(i64::MAX);
        *value = usize::try_from(signed_value + delta).unwrap_or(0);
    }
}

impl Histogram for ConsensusMetrics {
    fn add_point(&self, point: f64) {
        self.values
            .lock()
            .unwrap()
            .histograms
            .entry(self.prefix.clone())
            .or_default()
            .push(point);
    }
}

impl Label for ConsensusMetrics {
    fn set(&self, value: String) {
        *self
            .values
            .lock()
            .unwrap()
            .labels
            .entry(self.prefix.clone())
            .or_default() = value;
    }
}

impl ConsensusMetricsValue {
    /// Create a new instance of this [`ConsensusMetricsValue`] struct, setting all the counters and gauges
    #[must_use]
    pub fn new(metrics: &dyn Metrics) -> Self {
        Self {
            last_synced_block_height: metrics
                .create_gauge(String::from("last_synced_block_height"), None),
            last_decided_view: metrics.create_gauge(String::from("last_decided_view"), None),
            last_decided_time: metrics.create_gauge(String::from("last_decided_time"), None),
            current_view: metrics.create_gauge(String::from("current_view"), None),
            number_of_views_since_last_decide: metrics
                .create_gauge(String::from("number_of_views_since_last_decide"), None),
            number_of_views_per_decide_event: metrics
                .create_histogram(String::from("number_of_views_per_decide_event"), None),
            invalid_qc: metrics.create_gauge(String::from("invalid_qc"), None),
            outstanding_transactions: metrics
                .create_gauge(String::from("outstanding_transactions"), None),
            outstanding_transactions_memory_size: metrics
                .create_gauge(String::from("outstanding_transactions_memory_size"), None),
            number_of_timeouts: metrics.create_counter(String::from("number_of_timeouts"), None),
        }
    }
}

impl Default for ConsensusMetricsValue {
    fn default() -> Self {
        Self::new(&*NoMetrics::boxed())
    }
}

impl<TYPES: NodeType> Consensus<TYPES> {
    /// Update the current view.
    pub fn update_view(&mut self, view_number: TYPES::Time) {
        self.cur_view = view_number;
    }

    /// gather information from the parent chain of leafs
    /// # Errors
    /// If the leaf or its ancestors are not found in storage
    pub fn visit_leaf_ancestors<F>(
        &self,
        start_from: TYPES::Time,
        terminator: Terminator<TYPES::Time>,
        ok_when_finished: bool,
        mut f: F,
    ) -> Result<(), HotShotError<TYPES>>
    where
        F: FnMut(
            &Leaf<TYPES>,
            Arc<<TYPES as NodeType>::ValidatedState>,
            Option<Arc<<<TYPES as NodeType>::ValidatedState as ValidatedState<TYPES>>::Delta>>,
        ) -> bool,
    {
        let mut next_leaf = if let Some(view) = self.validated_state_map.get(&start_from) {
            view.get_leaf_commitment()
                .ok_or_else(|| HotShotError::InvalidState {
                    context: format!(
                        "Visited failed view {start_from:?} leaf. Expected successfuil leaf"
                    ),
                })?
        } else {
            return Err(HotShotError::InvalidState {
                context: format!("View {start_from:?} leaf does not exist in state map "),
            });
        };

        while let Some(leaf) = self.saved_leaves.get(&next_leaf) {
            let view = leaf.get_view_number();
            if let (Some(state), delta) = self.get_state_and_delta(view) {
                if let Terminator::Exclusive(stop_before) = terminator {
                    if stop_before == view {
                        if ok_when_finished {
                            return Ok(());
                        }
                        break;
                    }
                }
                next_leaf = leaf.get_parent_commitment();
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
                return Err(HotShotError::InvalidState {
                    context: format!("View {view:?} state does not exist in state map "),
                });
            }
        }
        Err(HotShotError::LeafNotFound {})
    }

    /// Garbage collects based on state change right now, this removes from both the
    /// `saved_payloads` and `validated_state_map` fields of `Consensus`.
    /// # Panics
    /// On inconsistent stored entries
    pub fn collect_garbage(&mut self, old_anchor_view: TYPES::Time, new_anchor_view: TYPES::Time) {
        // state check
        let anchor_entry = self
            .validated_state_map
            .iter()
            .next()
            .expect("INCONSISTENT STATE: anchor leaf not in state map!");
        if *anchor_entry.0 != old_anchor_view {
            error!(
                "Something about GC has failed. Older leaf exists than the previous anchor leaf."
            );
        }
        // perform gc
        self.saved_da_certs
            .retain(|view_number, _| *view_number >= old_anchor_view);
        self.saved_upgrade_certs
            .retain(|view_number, _| *view_number >= old_anchor_view);
        self.validated_state_map
            .range(old_anchor_view..new_anchor_view)
            .filter_map(|(_view_number, view)| view.get_leaf_commitment())
            .for_each(|leaf| {
                self.saved_leaves.remove(&leaf);
            });
        self.validated_state_map = self.validated_state_map.split_off(&new_anchor_view);
        self.saved_payloads = self.saved_payloads.split_off(&new_anchor_view);
    }

    /// Gets the last decided leaf.
    ///
    /// # Panics
    /// if the last decided view's leaf does not exist in the state map or saved leaves, which
    /// should never happen.
    #[must_use]
    pub fn get_decided_leaf(&self) -> Leaf<TYPES> {
        let decided_view_num = self.last_decided_view;
        let view = self.validated_state_map.get(&decided_view_num).unwrap();
        let leaf = view
            .get_leaf_commitment()
            .expect("Decided leaf not found! Consensus internally inconsistent");
        self.saved_leaves.get(&leaf).unwrap().clone()
    }

    /// Gets the validated state with the given view number, if in the state map.
    #[must_use]
    pub fn get_state(&self, view_number: TYPES::Time) -> Option<&Arc<TYPES::ValidatedState>> {
        match self.validated_state_map.get(&view_number) {
            Some(view) => view.get_state(),
            None => None,
        }
    }

    /// Gets the validated state and state delta with the given view number, if in the state map.
    #[must_use]
    pub fn get_state_and_delta(&self, view_number: TYPES::Time) -> StateAndDelta<TYPES> {
        match self.validated_state_map.get(&view_number) {
            Some(view) => view.get_state_and_delta(),
            None => (None, None),
        }
    }

    /// Gets the last decided validated state.
    ///
    /// # Panics
    /// If the last decided view's state does not exist in the state map, which should never
    /// happen.
    #[must_use]
    pub fn get_decided_state(&self) -> Arc<TYPES::ValidatedState> {
        let decided_view_num = self.last_decided_view;
        self.get_state_and_delta(decided_view_num)
            .0
            .expect("Decided state not found! Consensus internally inconsistent")
    }
}
