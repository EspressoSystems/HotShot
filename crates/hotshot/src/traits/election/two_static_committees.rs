// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{cmp::max, collections::BTreeMap, num::NonZeroU64};

use hotshot_types::{
    traits::{
        election::Membership,
        network::Topic,
        node_implementation::NodeType,
        signature_key::{SignatureKey, StakeTableEntryType},
    },
    PeerConfig,
};
use primitive_types::U256;
use utils::anytrace::Result;

/// Tuple type for eligible leaders
type EligibleLeaders<T> = (
    Vec<<<T as NodeType>::SignatureKey as SignatureKey>::StakeTableEntry>,
    Vec<<<T as NodeType>::SignatureKey as SignatureKey>::StakeTableEntry>,
);

/// Tuple type for stake tables
type StakeTables<T> = (
    Vec<<<T as NodeType>::SignatureKey as SignatureKey>::StakeTableEntry>,
    Vec<<<T as NodeType>::SignatureKey as SignatureKey>::StakeTableEntry>,
);

/// Tuple type for indexed stake tables
type IndexedStakeTables<T> = (
    BTreeMap<
        <T as NodeType>::SignatureKey,
        <<T as NodeType>::SignatureKey as SignatureKey>::StakeTableEntry,
    >,
    BTreeMap<
        <T as NodeType>::SignatureKey,
        <<T as NodeType>::SignatureKey as SignatureKey>::StakeTableEntry,
    >,
);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
/// The static committee election
pub struct TwoStaticCommittees<T: NodeType> {
    /// The nodes eligible for leadership.
    /// NOTE: This is currently a hack because the DA leader needs to be the quorum
    /// leader but without voting rights.
    eligible_leaders: EligibleLeaders<T>,

    /// The nodes on the committee and their stake
    stake_table: StakeTables<T>,

    /// The nodes on the committee and their stake, indexed by public key
    indexed_stake_table: IndexedStakeTables<T>,

    /// The network topic of the committee
    committee_topic: Topic,
}

impl<TYPES: NodeType> Membership<TYPES> for TwoStaticCommittees<TYPES> {
    type Error = utils::anytrace::Error;

    /// Create a new election
    fn new(
        eligible_leaders: Vec<PeerConfig<<TYPES as NodeType>::SignatureKey>>,
        committee_members: Vec<PeerConfig<<TYPES as NodeType>::SignatureKey>>,
        committee_topic: Topic,
    ) -> Self {
        // For each eligible leader, get the stake table entry
        let eligible_leaders: Vec<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry> =
            eligible_leaders
                .iter()
                .map(|member| member.stake_table_entry.clone())
                .filter(|entry| entry.stake() > U256::zero())
                .collect();

        let eligible_leaders1 = eligible_leaders
            .iter()
            .enumerate()
            .filter(|(idx, _)| idx % 2 == 0)
            .map(|(_, leader)| leader.clone())
            .collect();
        let eligible_leaders2 = eligible_leaders
            .iter()
            .enumerate()
            .filter(|(idx, _)| idx % 2 == 1)
            .map(|(_, leader)| leader.clone())
            .collect();

        // For each member, get the stake table entry
        let members: Vec<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry> =
            committee_members
                .iter()
                .map(|member| member.stake_table_entry.clone())
                .filter(|entry| entry.stake() > U256::zero())
                .collect();

        let members1: Vec<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry> = members
            .iter()
            .enumerate()
            .filter(|(idx, _)| idx % 2 == 0)
            .map(|(_, leader)| leader.clone())
            .collect();
        let members2: Vec<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry> = members
            .iter()
            .enumerate()
            .filter(|(idx, _)| idx % 2 == 1)
            .map(|(_, leader)| leader.clone())
            .collect();

        // Index the stake table by public key
        let indexed_stake_table1: BTreeMap<
            TYPES::SignatureKey,
            <TYPES::SignatureKey as SignatureKey>::StakeTableEntry,
        > = members1
            .iter()
            .map(|entry| (TYPES::SignatureKey::public_key(entry), entry.clone()))
            .collect();

        let indexed_stake_table2: BTreeMap<
            TYPES::SignatureKey,
            <TYPES::SignatureKey as SignatureKey>::StakeTableEntry,
        > = members2
            .iter()
            .map(|entry| (TYPES::SignatureKey::public_key(entry), entry.clone()))
            .collect();

        Self {
            eligible_leaders: (eligible_leaders1, eligible_leaders2),
            stake_table: (members1, members2),
            indexed_stake_table: (indexed_stake_table1, indexed_stake_table2),
            committee_topic,
        }
    }

    /// Get the stake table for the current view
    fn stake_table(
        &self,
        epoch: <TYPES as NodeType>::Epoch,
    ) -> Vec<<<TYPES as NodeType>::SignatureKey as SignatureKey>::StakeTableEntry> {
        if *epoch != 0 && *epoch % 2 == 0 {
            self.stake_table.0.clone()
        } else {
            self.stake_table.1.clone()
        }
    }

    /// Get all members of the committee for the current view
    fn committee_members(
        &self,
        _view_number: <TYPES as NodeType>::View,
        epoch: <TYPES as NodeType>::Epoch,
    ) -> std::collections::BTreeSet<<TYPES as NodeType>::SignatureKey> {
        if *epoch != 0 && *epoch % 2 == 0 {
            self.stake_table
                .0
                .iter()
                .map(TYPES::SignatureKey::public_key)
                .collect()
        } else {
            self.stake_table
                .1
                .iter()
                .map(TYPES::SignatureKey::public_key)
                .collect()
        }
    }

    /// Get all eligible leaders of the committee for the current view
    fn committee_leaders(
        &self,
        _view_number: <TYPES as NodeType>::View,
        epoch: <TYPES as NodeType>::Epoch,
    ) -> std::collections::BTreeSet<<TYPES as NodeType>::SignatureKey> {
        if *epoch != 0 && *epoch % 2 == 0 {
            self.eligible_leaders
                .0
                .iter()
                .map(TYPES::SignatureKey::public_key)
                .collect()
        } else {
            self.eligible_leaders
                .1
                .iter()
                .map(TYPES::SignatureKey::public_key)
                .collect()
        }
    }

    /// Get the stake table entry for a public key
    fn stake(
        &self,
        pub_key: &<TYPES as NodeType>::SignatureKey,
        epoch: <TYPES as NodeType>::Epoch,
    ) -> Option<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry> {
        // Only return the stake if it is above zero
        if *epoch != 0 && *epoch % 2 == 0 {
            self.indexed_stake_table.0.get(pub_key).cloned()
        } else {
            self.indexed_stake_table.1.get(pub_key).cloned()
        }
    }

    /// Check if a node has stake in the committee
    fn has_stake(
        &self,
        pub_key: &<TYPES as NodeType>::SignatureKey,
        epoch: <TYPES as NodeType>::Epoch,
    ) -> bool {
        if *epoch != 0 && *epoch % 2 == 0 {
            self.indexed_stake_table
                .0
                .get(pub_key)
                .is_some_and(|x| x.stake() > U256::zero())
        } else {
            self.indexed_stake_table
                .1
                .get(pub_key)
                .is_some_and(|x| x.stake() > U256::zero())
        }
    }

    /// Get the network topic for the committee
    fn committee_topic(&self) -> Topic {
        self.committee_topic.clone()
    }

    /// Index the vector of public keys with the current view number
    fn lookup_leader(
        &self,
        view_number: TYPES::View,
        epoch: <TYPES as NodeType>::Epoch,
    ) -> Result<TYPES::SignatureKey> {
        if *epoch != 0 && *epoch % 2 == 0 {
            #[allow(clippy::cast_possible_truncation)]
            let index = *view_number as usize % self.eligible_leaders.0.len();
            let res = self.eligible_leaders.0[index].clone();
            Ok(TYPES::SignatureKey::public_key(&res))
        } else {
            #[allow(clippy::cast_possible_truncation)]
            let index = *view_number as usize % self.eligible_leaders.1.len();
            let res = self.eligible_leaders.1[index].clone();
            Ok(TYPES::SignatureKey::public_key(&res))
        }
    }

    /// Get the total number of nodes in the committee
    fn total_nodes(&self, epoch: <TYPES as NodeType>::Epoch) -> usize {
        if *epoch != 0 && *epoch % 2 == 0 {
            self.stake_table.0.len()
        } else {
            self.stake_table.1.len()
        }
    }

    /// Get the voting success threshold for the committee
    fn success_threshold(&self) -> NonZeroU64 {
        NonZeroU64::new(((self.stake_table.0.len() as u64 * 2) / 3) + 1).unwrap()
    }

    /// Get the voting failure threshold for the committee
    fn failure_threshold(&self) -> NonZeroU64 {
        NonZeroU64::new(((self.stake_table.0.len() as u64) / 3) + 1).unwrap()
    }

    /// Get the voting upgrade threshold for the committee
    fn upgrade_threshold(&self) -> NonZeroU64 {
        NonZeroU64::new(max(
            (self.stake_table.0.len() as u64 * 9) / 10,
            ((self.stake_table.0.len() as u64 * 2) / 3) + 1,
        ))
        .unwrap()
    }
}
