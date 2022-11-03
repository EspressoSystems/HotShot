//! Utility functions, type aliases, helper structs and enum definitions.

use async_lock::Mutex;
use commit::Commitment;
use hotshot_types::{
    data::{Leaf, ViewNumber},
    error::HotShotError,
    message::ConsensusMessage,
    traits::{
        node_implementation::{NodeImplementation, TypeMap},
        Block, State,
    },
};
use hotshot_utils::channel::{unbounded, UnboundedReceiver, UnboundedSender};
use hotshot_utils::subscribable_rwlock::SubscribableRwLock;
use std::{
    collections::{BTreeMap, HashMap},
    ops::Deref,
    sync::{atomic::AtomicBool, Arc},
};

/// A view's state
#[derive(Debug)]
pub enum ViewInner<STATE: State> {
    /// Undecided view
    Leaf {
        /// Proposed leaf
        leaf: Commitment<Leaf<STATE>>,
    },
    /// Leaf has failed
    Failed,
}

impl<STATE: State> ViewInner<STATE> {
    /// return the underlying leaf hash if it exists
    #[must_use]
    pub fn get_leaf_commitment(&self) -> Option<&Commitment<Leaf<STATE>>> {
        if let Self::Leaf { leaf } = self {
            Some(leaf)
        } else {
            None
        }
    }
}

impl<STATE: State> Deref for View<STATE> {
    type Target = ViewInner<STATE>;

    fn deref(&self) -> &Self::Target {
        &self.view_inner
    }
}

/// struct containing messages for a view to send to replica
#[derive(Clone)]
pub struct ViewQueue<I: NodeImplementation> {
    /// to send networking events to Replica
    pub sender_chan: UnboundedSender<ConsensusMessage<I::StateType>>,

    /// to recv networking events for Replica
    pub receiver_chan: Arc<Mutex<UnboundedReceiver<ConsensusMessage<I::StateType>>>>,

    /// `true` if this queue has already received a proposal
    pub has_received_proposal: Arc<AtomicBool>,
}

impl<I: NodeImplementation> Default for ViewQueue<I> {
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
pub struct SendToTasks<I: NodeImplementation> {
    /// the current view number
    /// this should always be in sync with `Consensus`
    pub cur_view: ViewNumber,

    /// a map from view number to ViewQueue
    /// one of (replica|next leader)'s' task for view i will be listening on the channel in here
    pub channel_map: BTreeMap<ViewNumber, ViewQueue<I>>,
}

impl<I: NodeImplementation> SendToTasks<I> {
    /// create new sendtosasks
    #[must_use]
    pub fn new(view_num: ViewNumber) -> Self {
        SendToTasks {
            cur_view: view_num,
            channel_map: BTreeMap::default(),
        }
    }
}

/// This exists so we can perform state transitions mutably
#[derive(Debug)]
pub struct View<STATE: State> {
    /// The view data. Wrapped in a struct so we can mutate
    pub view_inner: ViewInner<STATE>,
}

/// The result used in this crate
pub type Result<T = ()> = std::result::Result<T, HotShotError>;

/// A struct containing information about a finished round.
#[derive(Debug, Clone)]
pub struct RoundFinishedEvent {
    /// The round that finished
    pub view_number: ViewNumber,
}

/// Locked wrapper around `TransactionHashMap`
pub type TransactionStorage<I> = Arc<SubscribableRwLock<TransactionHashMap<I>>>;

/// Map that stores transactions
pub type TransactionHashMap<I> = HashMap<
    Commitment<<<<I as NodeImplementation>::StateType as State>::BlockType as Block>::Transaction>,
    <I as TypeMap>::Transaction,
>;

/// Whether or not to stop inclusively or exclusively when walking
#[derive(Copy, Clone, Debug)]
pub enum Terminator {
    /// Stop right before this view number
    Exclusive(ViewNumber),
    /// Stop including this view number
    Inclusive(ViewNumber),
}
