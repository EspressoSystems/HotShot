//! Events that a `HotShot` instance can emit

use crate::{
    data::{Leaf, ViewNumber},
    error::HotShotError,
    traits::StateContents,
};
use std::sync::Arc;

/// A status event emitted by a `HotShot` instance
///
/// This includes some metadata, such as the stage and view number that the event was generated in,
/// as well as an inner [`EventType`] describing the event proper.
#[derive(Clone, Debug)]
pub struct Event<S: StateContents + Send + Sync> {
    /// The view number that this event originates from
    pub view_number: ViewNumber,
    /// The underlying event
    pub event: EventType<S>,
}

/// The type and contents of a status event emitted by a `HotShot` instance
///
/// This enum does not include metadata shared among all variants, such as the stage and view
/// number, and is thus always returned wrapped in an [`Event`].
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum EventType<S: StateContents> {
    /// A view encountered an error and was interrupted
    Error {
        /// The underlying error
        error: Arc<HotShotError>,
    },
    /// A new block was proposed
    Propose {
        /// The block that was proposed
        block: Arc<S::Block>,
    },
    /// A new decision event was issued
    Decide {
        /// The chain of Leafs that were committed by this decision
        ///
        /// This list is sorted in reverse view number order, with the newest (highest view number)
        /// block first in the list.
        ///
        /// This list may be incomplete if the node is currently performing catchup.
        leaf_chain: Arc<Vec<Leaf<S>>>,
    },
    /// A new view was started by this node
    NewView {
        /// The view being started
        view_number: ViewNumber,
    },
    /// A replica task was canceled by a timeout interrupt
    ReplicaViewTimeout {
        /// The view that timed out
        view_number: ViewNumber,
    },
    /// A next leader task was canceled by a timeout interrupt
    NextLeaderViewTimeout {
        /// The view that timed out
        view_number: ViewNumber,
    },
    /// This node is the leader for this view
    Leader {
        /// The current view number
        view_number: ViewNumber,
    },
    /// The node has been synced with the network
    Synced {
        /// The current view number
        view_number: ViewNumber,
    },
    /// The view has finished.  If values were decided on, a `Decide` event will also be emitted.
    ViewFinished {
        /// The view number that has just finished
        view_number: ViewNumber,
    },
}
