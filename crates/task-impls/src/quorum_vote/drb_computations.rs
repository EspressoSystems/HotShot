use std::collections::BTreeMap;

use hotshot_types::{
    drb::{compute_drb_result, DrbResult, DrbSeedInput},
    traits::node_implementation::{ConsensusTime, NodeType},
};
use tokio::{spawn, task::JoinHandle};

/// Number of previous results and seeds to keep
pub const KEEP_PREVIOUS_RESULT_COUNT: u64 = 8;

/// Helper struct to track state of DRB computations
pub struct DrbComputations<TYPES: NodeType> {
    /// Stored results from computations
    results: BTreeMap<TYPES::Epoch, DrbResult>,

    /// Currently live computation
    task: Option<(TYPES::Epoch, JoinHandle<DrbResult>)>,

    /// Stored inputs to computations
    seeds: BTreeMap<TYPES::Epoch, DrbSeedInput>,
}

impl<TYPES: NodeType> DrbComputations<TYPES> {
    #[must_use]
    /// Create a new DrbComputations
    pub fn new() -> Self {
        Self {
            results: BTreeMap::new(),
            task: None,
            seeds: BTreeMap::new(),
        }
    }

    /// If a task is currently live AND has finished, join it and save the result.
    /// Does not block if a live task has not finished.
    pub async fn try_join_task(&mut self) {
        if let Some((epoch, join_handle)) = &mut self.task {
            if join_handle.is_finished() {
                match join_handle.await {
                    Ok(result) => {
                        self.results.insert(*epoch, result);
                    }
                    Err(e) => {
                        tracing::error!("error joining DRB computation task: {e:?}");
                    }
                }
                self.task = None;
            }
        }
    }

    /// If a task is currently live AND has finished, join it and save the result.
    /// If a task is currently live and NOT finished, abort it.
    /// self.task will always be None after this call.
    pub async fn join_or_abort_task(&mut self) {
        if let Some((epoch, join_handle)) = &mut self.task {
            if join_handle.is_finished() {
                match join_handle.await {
                    Ok(result) => {
                        self.results.insert(*epoch, result);
                    }
                    Err(e) => {
                        tracing::error!("error joining DRB computation task: {e:?}");
                    }
                }
            } else {
                join_handle.abort();
            }
            self.task = None;
        }
    }

    /// Starts a new task. If a previous task is currently live, will attempt to join it (if finished), or abort it.
    pub async fn start_new_task(&mut self, epoch: TYPES::Epoch, drb_seed_input: DrbSeedInput) {
        self.join_or_abort_task().await;
        self.seeds.insert(epoch, drb_seed_input);
        let new_drb_task = spawn(async move { compute_drb_result::<TYPES>(drb_seed_input) });
        self.task = Some((epoch, new_drb_task));
    }

    /// Retrieves the result for a given epoch
    pub fn get_result(&self, epoch: TYPES::Epoch) -> Option<DrbResult> {
        self.results.get(&epoch).copied()
    }

    /// Retrieves the seed for a given epoch
    pub fn get_seed(&self, epoch: TYPES::Epoch) -> Option<DrbSeedInput> {
        self.seeds.get(&epoch).copied()
    }

    /// Garbage collects internal data structures
    pub fn garbage_collect(&mut self, epoch: TYPES::Epoch) {
        if epoch.u64() < KEEP_PREVIOUS_RESULT_COUNT {
            return;
        }

        let retain_epoch = epoch - KEEP_PREVIOUS_RESULT_COUNT;
        // N.B. x.split_off(y) returns the part of the map where key >= y

        // Remove result entries older than EPOCH
        self.results = self.results.split_off(&retain_epoch);

        // Remove result entries older than EPOCH+1
        self.seeds = self.seeds.split_off(&(retain_epoch + 1));
    }
}

impl<TYPES: NodeType> Default for DrbComputations<TYPES> {
    fn default() -> Self {
        Self::new()
    }
}
