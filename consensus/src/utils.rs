//! Utility functions, type aliases, helper structs and enum definitions.

use async_compatibility_layer::channel::{unbounded, UnboundedReceiver, UnboundedSender};
use async_lock::Mutex;
use commit::Commitment;
use hotshot_types::{
    data::{LeafBlock, LeafType},
    message::ConsensusMessageType,
    traits::node_implementation::{NodeImplementation, NodeType},
};
use std::{
    collections::BTreeMap,
    ops::Deref,
    sync::{atomic::AtomicBool, Arc},
};

/// A view's state
#[derive(Debug)]
pub enum ViewInner<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    /// A pending view with an available block but not leaf proposal yet.
    ///
    /// Storing this state allows us to garbage collect blocks for views where a proposal is never
    /// made. This saves memory when a leader fails and subverts a DoS attack where malicious
    /// leaders repeatedly request availability for blocks that they never propose.
    DA {
        /// Available block.
        block: Commitment<LeafBlock<LEAF>>,
    },
    /// Undecided view
    Leaf {
        /// Proposed leaf
        leaf: Commitment<LEAF>,
    },
    /// Leaf has failed
    Failed,
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> ViewInner<TYPES, LEAF> {
    /// return the underlying leaf hash if it exists
    #[must_use]
    pub fn get_leaf_commitment(&self) -> Option<Commitment<LEAF>> {
        if let Self::Leaf { leaf } = self {
            Some(*leaf)
        } else {
            None
        }
    }

    /// return the underlying block hash if it exists
    #[must_use]
    pub fn get_block_commitment(&self) -> Option<Commitment<LeafBlock<LEAF>>> {
        if let Self::DA { block } = self {
            Some(*block)
        } else {
            None
        }
    }
}

impl<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> Deref for View<TYPES, LEAF> {
    type Target = ViewInner<TYPES, LEAF>;

    fn deref(&self) -> &Self::Target {
        &self.view_inner
    }
}

/// struct containing messages for a view to send to a replica or DA committee member.
#[derive(Clone)]
pub struct ViewQueue<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// to send networking events to a replica or DA committee member.
    pub sender_chan: UnboundedSender<
        <I::ConsensusMessage as ConsensusMessageType<TYPES, I>>::ProcessedConsensusMessage,
    >,

    /// to recv networking events for a replica or DA committee member.
    pub receiver_chan: Arc<
        Mutex<
            UnboundedReceiver<
                <I::ConsensusMessage as ConsensusMessageType<TYPES, I>>::ProcessedConsensusMessage,
            >,
        >,
    >,

    /// `true` if this queue has already received a proposal
    pub has_received_proposal: Arc<AtomicBool>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> Default for ViewQueue<TYPES, I> {
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
pub struct SendToTasks<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// the current view number
    /// this should always be in sync with `Consensus`
    pub cur_view: TYPES::Time,

    /// a map from view number to ViewQueue
    /// one of (replica|next leader)'s' task for view i will be listening on the channel in here
    pub channel_map: BTreeMap<TYPES::Time, ViewQueue<TYPES, I>>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> SendToTasks<TYPES, I> {
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
pub struct View<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>> {
    /// The view data. Wrapped in a struct so we can mutate
    pub view_inner: ViewInner<TYPES, LEAF>,
}

/// A struct containing information about a finished round.
#[derive(Debug, Clone)]
pub struct RoundFinishedEvent<TYPES: NodeType> {
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
