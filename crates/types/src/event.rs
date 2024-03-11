//! Events that a `HotShot` instance can emit

use crate::{
    data::{DAProposal, Leaf, QuorumProposal, UpgradeProposal, VidDisperse},
    error::HotShotError,
    message::Proposal,
    simple_certificate::QuorumCertificate,
    traits::{node_implementation::NodeType, ValidatedState},
};

use std::sync::Arc;
/// A status event emitted by a `HotShot` instance
///
/// This includes some metadata, such as the stage and view number that the event was generated in,
/// as well as an inner [`EventType`] describing the event proper.
#[derive(Clone, Debug)]
pub struct Event<TYPES: NodeType> {
    /// The view number that this event originates from
    pub view_number: TYPES::Time,
    /// The underlying event
    pub event: EventType<TYPES>,
}

/// Decided leaf with the corresponding state and VID info.
#[derive(Clone, Debug)]
pub struct LeafInfo<TYPES: NodeType> {
    /// Decided leaf.
    pub leaf: Leaf<TYPES>,
    /// Validated state.
    pub state: Arc<<TYPES as NodeType>::ValidatedState>,
    /// Optional application-specific state delta.
    pub delta: Option<Arc<<<TYPES as NodeType>::ValidatedState as ValidatedState<TYPES>>::Delta>>,
    /// Optional VID disperse data.
    pub vid: Option<VidDisperse<TYPES>>,
}

impl<TYPES: NodeType> LeafInfo<TYPES> {
    /// Constructor.
    pub fn new(
        leaf: Leaf<TYPES>,
        state: Arc<<TYPES as NodeType>::ValidatedState>,
        delta: Option<Arc<<<TYPES as NodeType>::ValidatedState as ValidatedState<TYPES>>::Delta>>,
        vid: Option<VidDisperse<TYPES>>,
    ) -> Self {
        Self {
            leaf,
            state,
            delta,
            vid,
        }
    }
}

/// The chain of decided leaves with its corresponding state and VID info.
pub type LeafChain<TYPES> = Vec<LeafInfo<TYPES>>;

/// The type and contents of a status event emitted by a `HotShot` instance
///
/// This enum does not include metadata shared among all variants, such as the stage and view
/// number, and is thus always returned wrapped in an [`Event`].
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum EventType<TYPES: NodeType> {
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
        /// Vid Info for a decided view may be missing if this node never saw it's share.
        leaf_chain: Arc<LeafChain<TYPES>>,
        /// The QC signing the most recent leaf in `leaf_chain`.
        ///
        /// Note that the QC for each additional leaf in the chain can be obtained from the leaf
        /// before it using
        qc: Arc<QuorumCertificate<TYPES>>,
        /// Optional information of the number of transactions in the block, for logging purposes.
        block_size: Option<u64>,
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
    /// The view timed out
    ViewTimeout {
        /// The view that timed out
        view_number: TYPES::Time,
    },
    /// New transactions were received from the network
    /// or submitted to the network by us
    Transactions {
        /// The list of transactions
        transactions: Vec<TYPES::Transaction>,
    },
    /// DA proposal was received from the network
    /// or submitted to the network by us
    DAProposal {
        /// Contents of the proposal
        proposal: Proposal<TYPES, DAProposal<TYPES>>,
        /// Public key of the leader submitting the proposal
        sender: TYPES::SignatureKey,
    },
    /// Quorum proposal was received from the network
    /// or submitted to the network by us
    QuorumProposal {
        /// Contents of the proposal
        proposal: Proposal<TYPES, QuorumProposal<TYPES>>,
        /// Public key of the leader submitting the proposal
        sender: TYPES::SignatureKey,
    },
    /// Upgrade proposal was received from the network
    /// or submitted to the network by us
    UpgradeProposal {
        /// Contents of the proposal
        proposal: Proposal<TYPES, UpgradeProposal<TYPES>>,
        /// Public key of the leader submitting the proposal
        sender: TYPES::SignatureKey,
    },
}
