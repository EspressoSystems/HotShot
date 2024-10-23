use std::{
    marker::PhantomData,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use async_compatibility_layer::art::async_spawn;
use async_lock::Mutex;
use async_std::task::JoinHandle;

use crate::traits::node_implementation::{ConsensusTime, NodeType};

/// Storage for light data that doesn't require heavy locking. This is done to avoid
/// repeatedly locking the Consensus object for trivial state querying.
#[derive(custom_debug::Debug)]
struct TemporalState<TYPES: NodeType> {
    /// timeout task handle
    timeout_task: Mutex<JoinHandle<()>>, // TODO: Do we need to join on drop?

    /// View number that is currently on
    cur_view: AtomicU64,

    /// Epoch number that is currently on
    cur_epoch: AtomicU64,

    phantom: PhantomData<TYPES>,
}

impl<TYPES: NodeType> TemporalState<TYPES> {
    pub fn new(cur_view: TYPES::View, cur_epoch: TYPES::Epoch) -> Self {
        Self {
            timeout_task: Mutex::new(async_spawn(async {})),
            cur_view: AtomicU64::new(cur_view.u64()),
            cur_epoch: AtomicU64::new(cur_epoch.u64()),
            phantom: PhantomData,
        }
    }
}

/// Wrapper around a TemporalState, used to generate Readers and Writers.
#[derive(Clone)]
pub struct OuterTemporalState<TYPES: NodeType>(Arc<Box<TemporalState<TYPES>>>);

impl<TYPES: NodeType> OuterTemporalState<TYPES> {
    /// Constructs a new OuterTemporalState
    pub fn new(cur_view: TYPES::View, cur_epoch: TYPES::Epoch) -> Self {
        Self(Arc::new(Box::new(TemporalState::<TYPES>::new(
            cur_view, cur_epoch,
        ))))
    }

    /// Constructs a Writer to our TemporalState
    pub fn writer(&self) -> TemporalStateWriter<TYPES> {
        TemporalStateWriter::<TYPES> {
            state: self.clone(),
        }
    }

    /// Constructs a Reader to our TemporalState
    pub fn reader(&self) -> TemporalStateReader<TYPES> {
        TemporalStateReader::<TYPES> {
            state: self.clone(),
        }
    }

    /// Reads the value of cur_view
    pub fn cur_view(&self) -> TYPES::View {
        TYPES::View::new(self.0.cur_view.load(Ordering::Relaxed))
    }

    /// Reads the value of cur_epoch
    pub fn cur_epoch(&self) -> TYPES::Epoch {
        TYPES::Epoch::new(self.0.cur_epoch.load(Ordering::Relaxed))
    }

    /// Updates the value of cur_view
    pub fn update_cur_view(&mut self, value: TYPES::View) {
        self.0.cur_view.store(value.u64(), Ordering::Relaxed)
    }

    /// Updates the value of cur_epoch
    pub fn update_cur_epoch(&mut self, value: TYPES::Epoch) {
        self.0.cur_epoch.store(value.u64(), Ordering::Relaxed)
    }

    /// Updates the value of cur_view, returning the old value
    pub fn swap_cur_view(&mut self, value: TYPES::View) -> TYPES::View {
        TYPES::View::new(self.0.cur_view.swap(value.u64(), Ordering::Relaxed))
    }

    /// Updates the value of cur_epoch, returning the old value
    pub fn swap_cur_epoch(&mut self, value: TYPES::Epoch) -> TYPES::Epoch {
        TYPES::Epoch::new(self.0.cur_epoch.swap(value.u64(), Ordering::Relaxed))
    }
}

pub struct TemporalStateWriter<TYPES: NodeType> {
    state: OuterTemporalState<TYPES>,
}

#[derive(Clone)]
pub struct TemporalStateReader<TYPES: NodeType> {
    state: OuterTemporalState<TYPES>,
}

impl<TYPES: NodeType> TemporalStateWriter<TYPES> {
    /// Reads the value of cur_view
    pub fn cur_view(&self) -> TYPES::View {
        self.state.cur_view()
    }

    /// Reads the value of cur_epoch
    pub fn cur_epoch(&self) -> TYPES::Epoch {
        self.state.cur_epoch()
    }

    /// Updates the value of cur_view
    pub fn update_cur_view(&mut self, value: TYPES::View) {
        self.state.update_cur_view(value)
    }

    /// Updates the value of cur_epoch
    pub fn update_cur_epoch(&mut self, value: TYPES::Epoch) {
        self.state.update_cur_epoch(value)
    }

    /// Updates the value of cur_view, returning the old value
    pub fn swap_cur_view(&mut self, value: TYPES::View) -> TYPES::View {
        self.state.swap_cur_view(value)
    }

    /// Updates the value of cur_epoch, returning the old value
    pub fn swap_cur_epoch(&mut self, value: TYPES::Epoch) -> TYPES::Epoch {
        self.state.swap_cur_epoch(value)
    }

    /// Replaces the current timeout_task with a new JoinHandle, returning the old value
    pub async fn replace_timeout_task(
        &mut self,
        new_timeout_task: JoinHandle<()>,
    ) -> JoinHandle<()> {
        // TODO: Figure out if we should handle lock poisoning
        let mut guard = self.state.0.timeout_task.lock().await;
        std::mem::replace(&mut *guard, new_timeout_task)
    }
}

impl<TYPES: NodeType> TemporalStateReader<TYPES> {
    /// Reads the value of cur_view
    pub fn cur_view(&self) -> TYPES::View {
        self.state.cur_view()
    }

    /// Reads the value of cur_epoch
    pub fn cur_epoch(&self) -> TYPES::Epoch {
        self.state.cur_epoch()
    }
}
