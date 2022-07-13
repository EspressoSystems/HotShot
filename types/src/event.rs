//! Events that a `HotShot` instance can emit

use crate::{
    data::{Stage, VecQuorumCertificate, ViewNumber},
    error::HotShotError,
};
use std::sync::Arc;

/// A status event emitted by a `HotShot` instance
///
/// This includes some metadata, such as the stage and view number that the event was generated in,
/// as well as an inner [`EventType`] describing the event proper.
#[derive(Clone, Debug)]
pub struct Event<B: Send + Sync, S: Send + Sync> {
    /// The view number that this event originates from
    pub view_number: ViewNumber,
    /// The stage that this event originates from
    pub stage: Stage,
    /// The underlying event
    pub event: EventType<B, S>,
}

/// The type and contents of a status event emitted by a `HotShot` instance
///
/// This enum does not include metadata shared among all variants, such as the stage and view
/// number, and is thus always returned wrapped in an [`Event`].
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum EventType<B: Send + Sync, S: Send + Sync> {
    /// A view encountered an error and was interrupted
    Error {
        /// The underlying error
        error: Arc<HotShotError>,
    },
    /// A new block was proposed
    Propose {
        /// The block that was proposed
        block: Arc<B>,
    },
    /// A new decision event was issued
    Decide {
        /// The list of blocks that were committed by this decision
        ///
        /// This list is sorted in reverse view number order, with the newest (highest view number)
        /// block first in the list.
        ///
        /// This list may be incomplete if the node is currently performing catchup.
        block: Arc<Vec<B>>,
        /// The list of states that were committed by this decision
        ///
        /// This list is sorted in reverse view number order, with the newest (highest view number)
        /// state first in the list.
        ///
        /// This list may be incomplete if the node is currently performing catchup.
        state: Arc<Vec<S>>,
        /// The quorum certificates that accompy this Decide
        qcs: Arc<Vec<VecQuorumCertificate>>,
    },
    /// A new view was started by this node
    NewView {
        /// The view being started
        view_number: ViewNumber,
    },
    /// A view was canceled by a timeout interrupt
    ViewTimeout {
        /// The view that timed out
        view_number: ViewNumber,
    },
    /// This node is the leader for this view
    Leader {
        /// The current view number
        view_number: ViewNumber,
    },
    /// This node is a follower for this view
    Follower {
        /// The current view number
        view_number: ViewNumber,
    },

    /// The node has been synced with the network
    Synced {
        /// The current view number
        view_number: ViewNumber,
    },
}
