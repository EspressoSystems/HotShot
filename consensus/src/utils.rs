//! Utility functions, type aliases, helper structs and enum definitions.

use async_lock::Mutex;
use commit::Commitment;
use hotshot_types::{
    data::{LeafType, ProposalType},
    message::ConsensusMessage,
    traits::node_implementation::NodeTypes,
};
use hotshot_utils::channel::{unbounded, UnboundedReceiver, UnboundedSender};
use std::{
    collections::BTreeMap,
    ops::Deref,
    sync::{atomic::AtomicBool, Arc},
};

/// A view's state
#[derive(Debug)]
pub enum ViewInner<TYPES: NodeTypes, LEAF: LeafType<NodeType = TYPES>> {
    /// Undecided view
    Leaf {
        /// Proposed leaf
        leaf: Commitment<LEAF>,
    },
    /// Leaf has failed
    Failed,
}

impl<TYPES: NodeTypes, LEAF: LeafType<NodeType = TYPES>> ViewInner<TYPES, LEAF> {
    /// return the underlying leaf hash if it exists
    #[must_use]
    pub fn get_leaf_commitment(&self) -> Option<&Commitment<LEAF>> {
        if let Self::Leaf { leaf } = self {
            Some(leaf)
        } else {
            None
        }
    }
}

impl<TYPES: NodeTypes, LEAF: LeafType<NodeType = TYPES>> Deref for View<TYPES, LEAF> {
    type Target = ViewInner<TYPES, LEAF>;

    fn deref(&self) -> &Self::Target {
        &self.view_inner
    }
}

/// struct containing messages for a view to send to replica
#[derive(Clone)]
pub struct ViewQueue<
    TYPES: NodeTypes,
    LEAF: LeafType<NodeType = TYPES>,
    PROPOSAL: ProposalType<NodeTypes = TYPES>,
> {
    /// to send networking events to Replica
    pub sender_chan: UnboundedSender<ConsensusMessage<TYPES, LEAF, PROPOSAL>>,

    /// to recv networking events for Replica
    pub receiver_chan: Arc<Mutex<UnboundedReceiver<ConsensusMessage<TYPES, LEAF, PROPOSAL>>>>,

    /// `true` if this queue has already received a proposal
    pub has_received_proposal: Arc<AtomicBool>,
}

impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
    > Default for ViewQueue<TYPES, LEAF, PROPOSAL>
{
    /// create new view queue
    fn default() -> Self {
        let (s, r) = unbounded();
        ViewQueue {
            sender_chan: s,
            receiver_chan: Arc::new(Mutex::new(r)),
            has_received_proposal: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// metadata for sending information to replica (and in the future, the leader)
pub struct SendToTasks<
    TYPES: NodeTypes,
    LEAF: LeafType<NodeType = TYPES>,
    PROPOSAL: ProposalType<NodeTypes = TYPES>,
> {
    /// the current view number
    /// this should always be in sync with `Consensus`
    pub cur_view: TYPES::Time,

    /// a map from view number to ViewQueue
    /// one of (replica|next leader)'s' task for view i will be listening on the channel in here
    pub channel_map: BTreeMap<TYPES::Time, ViewQueue<TYPES, LEAF, PROPOSAL>>,
}

impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
    > SendToTasks<TYPES, LEAF, PROPOSAL>
{
    /// create new sendtosasks
    #[must_use]
    pub fn new(view_num: TYPES::Time) -> Self {
        SendToTasks {
            cur_view: view_num,
            channel_map: BTreeMap::default(),
        }
    }
}

/// This exists so we can perform state transitions mutably
#[derive(Debug)]
pub struct View<TYPES: NodeTypes, LEAF: LeafType<NodeType = TYPES>> {
    /// The view data. Wrapped in a struct so we can mutate
    pub view_inner: ViewInner<TYPES, LEAF>,
}

/// A struct containing information about a finished round.
#[derive(Debug, Clone)]
pub struct RoundFinishedEvent<TYPES: NodeTypes> {
    /// The round that finished
    pub view_number: TYPES::Time,
}

/// Whether or not to stop inclusively or exclusively when walking
#[derive(Copy, Clone, Debug)]
pub enum Terminator<T> {
    /// Stop right before this view number
    Exclusive(T),
    /// Stop including this view number
    Inclusive(T),
}
