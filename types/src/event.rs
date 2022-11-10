//! Events that a `HotShot` instance can emit

use crate::{data::Leaf, error::HotShotError, traits::node_implementation::NodeTypes};
use std::sync::Arc;

/// A status event emitted by a `HotShot` instance
///
/// This includes some metadata, such as the stage and view number that the event was generated in,
/// as well as an inner [`EventType`] describing the event proper.
#[derive(Clone, Debug)]
pub struct Event<TYPES: NodeTypes> {
    /// The view number that this event originates from
    pub view_number: TYPES::Time,
    /// The underlying event
    pub event: EventType<TYPES>,
}

/// The type and contents of a status event emitted by a `HotShot` instance
///
/// This enum does not include metadata shared among all variants, such as the stage and view
/// number, and is thus always returned wrapped in an [`Event`].
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum EventType<TYPES: NodeTypes> {
    /// A view encountered an error and was interrupted
    Error {
        /// The underlying error
        error: Arc<HotShotError<TYPES>>,
    },
    /// A new decision event was issued
    Decide {
        /// The chain of Leafs that were committed by this decision
        ///
        /// This list is sorted in reverse view number order, with the newest (highest view number)
        /// block first in the list.
        ///
        /// This list may be incomplete if the node is currently performing catchup.
        leaf_chain: Arc<Vec<Leaf<TYPES>>>,
    },
    /// A replica task was canceled by a timeout interrupt
    ReplicaViewTimeout {
        /// The view that timed out
        view_number: TYPES::Time,
    },
    /// A next leader task was canceled by a timeout interrupt
    NextLeaderViewTimeout {
        /// The view that timed out
        view_number: TYPES::Time,
    },
    /// The view has finished.  If values were decided on, a `Decide` event will also be emitted.
    ViewFinished {
        /// The view number that has just finished
        view_number: TYPES::Time,
    },
}
