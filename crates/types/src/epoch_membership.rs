use std::{collections::HashMap, sync::Arc};

use async_broadcast::{broadcast, InactiveReceiver};
use async_lock::{Mutex, RwLock};

use crate::traits::node_implementation::NodeType;

#[derive(Clone, Debug)]
/// Struct to signal a catchup of a membership is complete
/// Used internally in `EpochMembershipCoordinator` only
struct CatchupSignal {}

/// Struct to Coordinate membership catchup
pub struct EpochMembershipCoordinator<TYPES: NodeType> {
    /// The underlying membhersip
    membership: Arc<RwLock<TYPES::Membership>>,

    /// Any in progress attempts at catching up are stored in this map
    /// Any new callers wantin an `EpochMembership` will await on the signal
    /// alerting them the membership is ready.  The first caller for an epoch will
    /// wait for the actual catchup and allert future callers when it's done
    catchup_map: Arc<Mutex<HashMap<u64, InactiveReceiver<CatchupSignal>>>>,
}

impl<TYPES: NodeType> EpochMembershipCoordinator<TYPES> {
    /// Get a Membership for a given Epoch, which is guarenteed to have a stake
    /// table for the given Epoch
    pub async fn membership_for_epoch(&self, epoch: u64) -> EpochMembership<TYPES> {
        // CHECK IF MEMBERSHIP HAS EPOCH FIRST
        let mut map = self.catchup_map.lock().await;
        if let Some(inactive_rx) = map.get(&epoch) {
            let mut rx = inactive_rx.activate_cloned();
            drop(map);
            let _ = rx.recv().await;
        } else {
            let (tx, rx) = broadcast(1);
            map.insert(epoch, rx.deactivate());
            drop(map);
            // do catchup
            let _ = tx.broadcast_direct(CatchupSignal {}).await;
        };

        EpochMembership {
            epoch,
            membership: Arc::clone(&self.membership),
        }
    }
}

/// Wrapper around a membership that guarentees that the epoch
/// has a stake table
pub struct EpochMembership<TYPES: NodeType> {
    /// Epoch the `membership` is guarenteed to have a stake table for
    pub epoch: u64,
    /// Underlying membership
    pub membership: Arc<RwLock<TYPES::Membership>>,
}
