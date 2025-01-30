use std::{collections::HashMap, sync::Arc};

use async_broadcast::{broadcast, InactiveReceiver};
use async_lock::{Mutex, RwLock};
use utils::anytrace::Result;

use crate::traits::election::Membership;
use crate::traits::node_implementation::{ConsensusTime, NodeType};
use crate::utils::root_block_in_epoch;

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
    catchup_map: Arc<Mutex<HashMap<TYPES::Epoch, InactiveReceiver<CatchupSignal>>>>,

    /// Number of blocks in an epoch
    epoch_height: u64,
}

impl<TYPES: NodeType> EpochMembershipCoordinator<TYPES> {
    /// Get a Membership for a given Epoch, which is guarenteed to have a stake
    /// table for the given Epoch
    pub async fn membership_for_epoch(&self, epoch: TYPES::Epoch) -> EpochMembership<TYPES> {
        if self.membership.read().await.has_epoch(epoch).await {
            return EpochMembership {
                epoch,
                membership: Arc::clone(&self.membership),
            };
        }
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
            self.catchup(epoch).await;
            let _ = tx.broadcast_direct(CatchupSignal {}).await;
        };

        EpochMembership {
            epoch,
            membership: Arc::clone(&self.membership),
        }
    }

    /// Catches the membership up to the epoch passed as an argument.  
    /// To do this try to get the stake table for the epoch containing this epoch's root
    /// if the root does not exist recusively catchup until you've found it
    ///
    /// If there is another catchup in progress this will not duplicate efforts
    /// e.g. if we start with only epoch 0 stake table and call catchup for epoch 10, then call catchup for epoch 20
    /// the first caller will actually do the work for to catchup to epoch 10 then the second caller will continue
    /// catching up to epoch 20
    async fn catchup(&self, epoch: TYPES::Epoch) {
        // recursively catchup until we have a stake table for the epoch containing our root
        assert!(
            *epoch != 0,
            "We are trying to catchup to epoch 0! This means the initial stake table is missing!"
        );
        let root_epoch = if *epoch == 1 {
            TYPES::Epoch::new(*epoch - 1)
        } else {
            TYPES::Epoch::new(*epoch - 2)
        };
        let root_membership = self.membership_for_epoch(root_epoch).await;

        // Get the epoch root headers and update our membership with them, finally sync them
        // Verification of the root is handled in get_epoch_root
        let Ok((next_epoch, header)) = root_membership
            .get_epoch_root(root_block_in_epoch(*root_epoch, self.epoch_height))
            .await
        else {
            return;
        };
        let Some(updater) = self
            .membership
            .read()
            .await
            .add_epoch_root(next_epoch, header)
            .await
        else {
            return;
        };
        updater(&mut *(self.membership.write().await));

        let Some(sync) = self.membership.read().await.sync_l1().await else {
            return;
        };
        sync(&mut *(self.membership.write().await));
    }
}

/// Wrapper around a membership that guarentees that the epoch
/// has a stake table
pub struct EpochMembership<TYPES: NodeType> {
    /// Epoch the `membership` is guarenteed to have a stake table for
    pub epoch: TYPES::Epoch,
    /// Underlying membership
    pub membership: Arc<RwLock<TYPES::Membership>>,
}

impl<TYPES: NodeType> EpochMembership<TYPES> {
    /// Wraps the same named Membership trait fn
    async fn get_epoch_root(
        &self,
        block_height: u64,
    ) -> Result<(TYPES::Epoch, TYPES::BlockHeader)> {
        self.membership
            .read()
            .await
            .get_epoch_root(block_height)
            .await
    }
    // TODO add function wrappers
}
