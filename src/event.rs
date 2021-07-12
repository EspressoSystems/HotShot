use crate::{data::Stage, PhaseLockError};

use std::sync::Arc;

/// A status event emitted by a `PhaseLock` instance
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct Event<B: Send + Sync, S: Send + Sync> {
    /// The view number that this event originates from
    pub view_number: u64,
    /// The stage that this event originates from
    pub stage: Stage,
    /// The underlying event
    pub event: EventType<B, S>,
}

/// The types of event that can be emitted by a `PhaseLock` instance
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum EventType<B: Send + Sync, S: Send + Sync> {
    /// An error occurred and the round was not completed
    Error {
        /// The underlying error
        error: Arc<PhaseLockError>,
    },
    /// A new block was proposed
    Propose {
        /// The block that was proposed
        block: Arc<B>,
    },
    /// A new state was decided on
    Decide {
        /// The block that was decided on
        block: Arc<B>,
        /// The new resulting state
        state: Arc<S>,
    },
    /// A new view was started by this nodes
    NewView {
        /// The view being started
        view_number: u64,
    },
    /// A view timed out and was interrupted
    ViewTimeout {
        /// The view that timed out
        view_number: u64,
    },
    /// The current node is the leader for this view
    Leader {
        /// The current view number
        view_number: u64,
    },
    /// The current node is a follower for this view
    Follower {
        /// The current view number
        view_number: u64,
    },
}
