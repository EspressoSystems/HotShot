// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! The election trait, used to decide which node is the leader and determine if a vote is valid.
use std::{collections::BTreeSet, fmt::Debug, num::NonZeroU64};

use utils::anytrace::Result;

use super::node_implementation::NodeType;
use crate::{traits::signature_key::SignatureKey, PeerConfig};

/// A protocol for determining membership in and participating in a committee.
pub trait Membership<TYPES: NodeType>: Clone + Debug + Send + Sync {
    /// The error type returned by methods like `lookup_leader`.
    type Error: std::fmt::Display;

    /// Create a committee
    fn new(
        // Note: eligible_leaders is currently a hack because the DA leader == the quorum leader
        // but they should not have voting power.
        stake_committee_members: Vec<PeerConfig<TYPES::SignatureKey>>,
        da_committee_members: Vec<PeerConfig<TYPES::SignatureKey>>,
    ) -> Self;

    /// Get all participants in the committee (including their stake) for a specific epoch
    fn stake_table(
        &self,
        epoch: TYPES::Epoch,
    ) -> Vec<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry>;

    /// Get all participants in the committee (including their stake) for a specific epoch
    fn da_stake_table(
        &self,
        epoch: TYPES::Epoch,
    ) -> Vec<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry>;

    /// Get all participants in the committee for a specific view for a specific epoch
    fn committee_members(
        &self,
        view_number: TYPES::View,
        epoch: TYPES::Epoch,
    ) -> BTreeSet<TYPES::SignatureKey>;

    /// Get all participants in the committee for a specific view for a specific epoch
    fn da_committee_members(
        &self,
        view_number: TYPES::View,
        epoch: TYPES::Epoch,
    ) -> BTreeSet<TYPES::SignatureKey>;

    /// Get all leaders in the committee for a specific view for a specific epoch
    fn committee_leaders(
        &self,
        view_number: TYPES::View,
        epoch: TYPES::Epoch,
    ) -> BTreeSet<TYPES::SignatureKey>;

    /// Get the stake table entry for a public key, returns `None` if the
    /// key is not in the table for a specific epoch
    fn stake(
        &self,
        pub_key: &TYPES::SignatureKey,
        epoch: TYPES::Epoch,
    ) -> Option<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry>;

    /// Get the DA stake table entry for a public key, returns `None` if the
    /// key is not in the table for a specific epoch
    fn da_stake(
        &self,
        pub_key: &TYPES::SignatureKey,
        epoch: TYPES::Epoch,
    ) -> Option<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry>;

    /// See if a node has stake in the committee in a specific epoch
    fn has_stake(&self, pub_key: &TYPES::SignatureKey, epoch: TYPES::Epoch) -> bool;

    /// See if a node has stake in the committee in a specific epoch
    fn has_da_stake(&self, pub_key: &TYPES::SignatureKey, epoch: TYPES::Epoch) -> bool;

    /// The leader of the committee for view `view_number` in `epoch`.
    ///
    /// Note: this function uses a HotShot-internal error type.
    /// You should implement `lookup_leader`, rather than implementing this function directly.
    ///
    /// # Errors
    /// Returns an error if the leader cannot be calculated.
    fn leader(&self, view: TYPES::View, epoch: TYPES::Epoch) -> Result<TYPES::SignatureKey> {
        use utils::anytrace::*;

        self.lookup_leader(view, epoch).wrap().context(info!(
            "Failed to get leader for view {view} in epoch {epoch}"
        ))
    }

    /// The leader of the committee for view `view_number` in `epoch`.
    ///
    /// Note: There is no such thing as a DA leader, so any consumer
    /// requiring a leader should call this.
    ///
    /// # Errors
    /// Returns an error if the leader cannot be calculated
    fn lookup_leader(
        &self,
        view: TYPES::View,
        epoch: TYPES::Epoch,
    ) -> std::result::Result<TYPES::SignatureKey, Self::Error>;

    /// Returns the number of total nodes in the committee in an epoch `epoch`
    fn total_nodes(&self, epoch: TYPES::Epoch) -> usize;

    /// Returns the number of total DA nodes in the committee in an epoch `epoch`
    fn da_total_nodes(&self, epoch: TYPES::Epoch) -> usize;

    /// Returns the threshold for a specific `Membership` implementation
    fn success_threshold(&self, epoch: TYPES::Epoch) -> NonZeroU64;

    /// Returns the DA threshold for a specific `Membership` implementation
    fn da_success_threshold(&self, epoch: TYPES::Epoch) -> NonZeroU64;

    /// Returns the threshold for a specific `Membership` implementation
    fn failure_threshold(&self, epoch: TYPES::Epoch) -> NonZeroU64;

    /// Returns the threshold required to upgrade the network protocol
    fn upgrade_threshold(&self, epoch: TYPES::Epoch) -> NonZeroU64;
}
