// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::collections::BTreeMap;
use std::num::NonZeroU64;

use hotshot_types::{
    traits::{
        election::Membership, network::Topic, node_implementation::NodeType,
        signature_key::SignatureKey,
    },
    PeerConfig,
};
#[cfg(feature = "randomized-leader-election")]
use rand::{rngs::StdRng, Rng};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]

/// The static committee election
pub struct StaticCommitteeLeaderForTwoViews<T: NodeType> {
    /// The nodes on the committee and their stake
    stake_table: Vec<<T::SignatureKey as SignatureKey>::StakeTableEntry>,

    /// The nodes on the committee and their stake, indexed by public key
    indexed_stake_table:
        BTreeMap<T::SignatureKey, <T::SignatureKey as SignatureKey>::StakeTableEntry>,

    // /// The members of the committee
    // committee_members: BTreeSet<T::SignatureKey>,
    #[cfg(feature = "fixed-leader-election")]
    /// The number of fixed leaders for gpuvid
    fixed_leader_for_gpuvid: usize,

    /// The network topic of the committee
    committee_topic: Topic,
}

impl<TYPES: NodeType> Membership<TYPES> for StaticCommitteeLeaderForTwoViews<TYPES> {
    /// Create a new election
    fn new(
        committee_members: Vec<PeerConfig<<TYPES as NodeType>::SignatureKey>>,
        committee_topic: Topic,
    ) -> Self {
        // For each member, get the stake table entry
        let members: Vec<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry> =
            committee_members
                .iter()
                .map(|member| member.stake_table_entry.clone())
                .collect();

        // Index the stake table by public key
        let indexed_stake_table: BTreeMap<
            TYPES::SignatureKey,
            <TYPES::SignatureKey as SignatureKey>::StakeTableEntry,
        > = members
            .iter()
            .map(|entry| (TYPES::SignatureKey::public_key(entry), entry.clone()))
            .collect();

        Self {
            stake_table: members,
            indexed_stake_table,
            committee_topic,
        }
    }

    fn get_stake_table(
        &self,
    ) -> Vec<<<TYPES as NodeType>::SignatureKey as SignatureKey>::StakeTableEntry> {
        self.stake_table.clone()
    }

    fn get_committee_members(
        &self,
        _view_number: <TYPES as NodeType>::Time,
    ) -> std::collections::BTreeSet<<TYPES as NodeType>::SignatureKey> {
        self.stake_table
            .iter()
            .map(TYPES::SignatureKey::public_key)
            .collect()
    }

    fn has_stake(&self, _pub_key: &<TYPES as NodeType>::SignatureKey) -> bool {
        todo!()
    }

    /// Get the network topic for the committee
    fn committee_topic(&self) -> Topic {
        self.committee_topic.clone()
    }

    /// Index the vector of public keys with the current view number
    fn get_leader(&self, view_number: TYPES::Time) -> TYPES::SignatureKey {
        let index = usize::try_from((*view_number / 2) % self.stake_table.len() as u64).unwrap();
        let res = self.stake_table[index].clone();
        TYPES::SignatureKey::public_key(&res)
    }

    fn stake(
        &self,
        pub_key: &<TYPES as NodeType>::SignatureKey,
    ) -> Option<<TYPES::SignatureKey as SignatureKey>::StakeTableEntry> {
        self.indexed_stake_table.get(pub_key).cloned()
    }

    fn total_nodes(&self) -> usize {
        self.stake_table.len()
    }

    fn success_threshold(&self) -> NonZeroU64 {
        NonZeroU64::new(((self.stake_table.len() as u64 * 2) / 3) + 1).unwrap()
    }

    fn failure_threshold(&self) -> NonZeroU64 {
        NonZeroU64::new(((self.stake_table.len() as u64) / 3) + 1).unwrap()
    }

    fn upgrade_threshold(&self) -> NonZeroU64 {
        NonZeroU64::new(((self.stake_table.len() as u64 * 9) / 10) + 1).unwrap()
    }
}
