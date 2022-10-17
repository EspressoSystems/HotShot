use commit::Commitment;
use hotshot_types::{
    data::{Leaf, ViewNumber},
    traits::{
        election::{Checked, Election, ElectionError, VoteToken},
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
        state::ConsensusTime,
        State,
    },
};
use hotshot_utils::hack::nll_todo;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, marker::PhantomData, num::NonZeroU64};
use tracing::{error, warn};

use super::static_committee::StaticElectionConfig;

/// Output of the simulated VRF
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub struct HashVrf(u128);

/// Key of the simulated VRF
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash)]
pub struct HashVrfKey(pub [u8; 32]);

impl HashVrfKey {
    /// Generate a random hash vrf key
    pub fn random() -> Self {
        let mut buffer = [0_u8; 32];
        rand::thread_rng().fill_bytes(&mut buffer[..]);
        Self(buffer)
    }
    /// Generates a hash vrf key from an arbitrary string of bytes
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Self {
        let bytes = bytes.as_ref();
        let hash = *blake3::hash(bytes).as_bytes();
        Self(hash)
    }
    /// Generates from a seed with an index
    pub fn generated_from_seed_indexed(seed: [u8; 32], index: u64) -> Self {
        let mut buffer = [0_u8; 40];
        buffer[0..32].copy_from_slice(&seed[..]);
        buffer[32..].copy_from_slice(&index.to_le_bytes());
        Self::from_bytes(buffer)
    }
}

impl SignatureKey for HashVrfKey {
    type PrivateKey = HashVrfKey;

    fn validate(&self, signature: &EncodedSignature, data: &[u8]) -> bool {
        // Not constant time, but this isn't secure anyway
        &Self::sign(self, data) == signature
    }

    fn sign(private_key: &Self::PrivateKey, data: &[u8]) -> EncodedSignature {
        // Make a hash
        let hash = blake3::keyed_hash(&private_key.0, data);
        // Wrap it
        EncodedSignature(hash.as_bytes().to_vec())
    }

    fn from_private(private_key: &Self::PrivateKey) -> Self {
        *private_key
    }

    fn to_bytes(&self) -> EncodedPublicKey {
        EncodedPublicKey(self.0.to_vec())
    }

    fn from_bytes(bytes: &EncodedPublicKey) -> Option<Self> {
        match (bytes.0[..]).try_into() {
            Ok(x) => Some(Self(x)),
            Err(_) => None,
        }
    }
    fn generated_from_seed_indexed(seed: [u8; 32], index: u64) -> (Self, Self::PrivateKey) {
        let k = HashVrfKey::generated_from_seed_indexed(seed, index);
        (k.clone(), k)
    }
}

/// Seed for the simulated VRF
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub struct HashVrfSeed(pub [u8; 32]);

impl HashVrfSeed {
    /// Generate a random hash vrf seed
    pub fn random() -> Self {
        let mut buffer = [0_u8; 32];
        rand::thread_rng().fill_bytes(&mut buffer[..]);
        Self(buffer)
    }
    /// Generates a hash vrf seed from an arbitrary string of bytes
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Self {
        let bytes = bytes.as_ref();
        let hash = *blake3::hash(bytes).as_bytes();
        Self(hash)
    }
}

/// HMAC-based simulation of a VRF
pub struct HashElection<S> {
    /// The stake table of this `HashElection`
    stake_table: BTreeMap<HashVrfKey, u64>,
    /// The seed for this `HashElection`
    seed: HashVrfSeed,
    /// TODO document
    pd: PhantomData<S>,

    /// Expected election size
    expected_election_size: NonZeroU64,
    /// Total possible participants
    total_participants: NonZeroU64,
}

impl<S> HashElection<S> {
    /// Create a new `HashElection` given a seed and a sate table
    pub fn new(
        stake_table: impl IntoIterator<Item = (HashVrfKey, u64)>,
        seed: HashVrfSeed,
        expected_election_size: NonZeroU64,
        total_participants: NonZeroU64,
    ) -> Self {
        Self {
            stake_table: stake_table.into_iter().collect(),
            seed,
            pd: PhantomData,
            expected_election_size,
            total_participants,
        }
    }

    /// Calculate the leader for the view using weighted random selection
    fn select_leader(&self, table: &BTreeMap<HashVrfKey, u64>, view: ViewNumber) -> HashVrfKey {
        // Note that BTreeMap will always iterate in the same order on every machine,
        // assuming the key type's `Ord` implementation is correct.
        // If you encounter issues that lead you here, check the key type's `Ord` implementation.

        // first we get the total weight of all keys
        let total_weight: u128 = table.iter().map(|(_, weight)| u128::from(*weight)).sum();

        // Hash some stuff up
        // - The current view number
        // - The seed
        // - The total weight of all entries
        let mut hasher = blake3::Hasher::default();
        hasher.update(&view.to_le_bytes());
        hasher.update(&self.seed.0);
        hasher.update(&total_weight.to_le_bytes());
        // Extract the output into a u128 by taking the first 16 bytes
        let hash = *hasher.finalize().as_bytes();
        let rand = u128::from_le_bytes(hash[..16].try_into().unwrap());

        // Modulo bias doesn't matter here, as this is a testing only implementation, and there
        // can't be more nodes than will fit into memory
        let index = rand % total_weight;
        // now we have an index, we'll have to iterate over the `table` entries until `0 <= index < weight`
        // after each iteration we subtract the weight of that entry by the index
        // this way we should get the index'nth key
        let mut current_index = index;
        for (key, weight) in table {
            let weight = u128::from(*weight);
            if weight < current_index {
                error!("Leader for this round is: {key:?}");
                return *key;
            }
            current_index -= weight;
        }
        // this should never be reached because `index` should always be smaller than the `total_weight`
        #[allow(clippy::panic)]
        {
            panic!("index is {index} but list total length is only {total_weight}");
        }
    }

    /// Generate a vote hash
    fn vote_hash(&self, key: &HashVrfKey, view: ViewNumber, vote_index: u64) -> u128 {
        // make the buffer
        let mut buf = Vec::<u8>::new();
        buf.extend(&self.seed.0);
        buf.extend(view.to_le_bytes());
        buf.extend(vote_index.to_le_bytes());
        // Make the hash
        let signature = HashVrfKey::sign(key, &buf);
        // Extract the upper bits
        u128::from_le_bytes(signature.0[..16].try_into().unwrap())
    }
    /// Generates the valid vote hashes
    fn vote_hashes(
        &self,
        key: &HashVrfKey,
        view: ViewNumber,
        votes: u64,
        selection_threshold: u128,
    ) -> Vec<u128> {
        (0_u64..votes)
            .map(|x| self.vote_hash(key, view, x))
            .filter(|x| x <= &selection_threshold)
            .collect()
    }
    /// Calculate the selection threshold based on the expected size and total participants
    fn calculate_selection_threshold(&self) -> u128 {
        let total_participants = u128::from(self.total_participants.get());
        let expected_size = u128::from(self.expected_election_size.get());
        // We want the probability of a given participant to be 1 / (total_participants * expected_size)
        // This means we need the selection threshold to be u128::MAX * (1 / (total_participants * expected_size))
        // This rearranges to: u128::MAX / (total_participants * expected_size)
        let output = u128::MAX / (total_participants * expected_size);
        warn!("Selection threshold calculated, {} {}", u128::MAX, output);
        output
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HashElectionVoteToken {
    key: HashVrfKey,
    stake: u64,
    // TODO (ct) is this the right name?
    valid_stake: u64,
}

impl VoteToken for HashElectionVoteToken {
    fn vote_count(&self) -> u64 {
        nll_todo()
    }
}

impl<S, T> Election<HashVrfKey, T> for HashElection<S>
where
    T: ConsensusTime,
    S: State,
{
    /// Mapping of a public key to the number of allowed voting attempts and the associated
    /// `HashVrfKey`
    type StakeTable = BTreeMap<HashVrfKey, u64>;
    type StateType = S;
    type VoteTokenType = HashElectionVoteToken;

    fn get_stake_table(
        &self,
        view_number: ViewNumber,
        _state: &Self::StateType,
    ) -> Self::StakeTable {
        self.stake_table.clone()
    }

    fn get_leader(&self, view_number: ViewNumber) -> HashVrfKey {
        self.select_leader(&self.stake_table, view_number)
    }



    // fn get_votes(
    //     &self,
    //     view_number: ViewNumber,
    //     pub_key: HashVrfKey,
    //     token: Self::VoteToken,
    //     _next_state: Commitment<Leaf<Self::StateType>>,
    // ) -> std::result::Result<Self::VoteToken, ElectionError> {
    //     warn!("Validating vote token");
    //     let selection_threshold = self.calculate_selection_threshold();
    //
    //     let hashes = self.vote_hashes(&token.0, view_number, token.1, selection_threshold);
    //     match self.stake_table.get(&pub_key) {
    //         Some(votes) => {
    //             if &token.1 == votes && !hashes.is_empty() {
    //                 Ok((token.0, token.1, hashes.len().try_into().unwrap()))
    //             } else {
    //                 ElectionError::StubError
    //             }
    //         }
    //         None => ElectionError::StubError,
    //     }
    // }

    // fn get_vote_count(&self, token: &Self::ValidatedVoteToken) -> u64 {
    //     token.2
    // }

    fn make_vote_token(
        &self,
        view_number: ViewNumber,
        private_key: &HashVrfKey,
        _next_state: Commitment<Leaf<Self::StateType>>,
    ) -> Result<Option<Self::VoteTokenType>, ElectionError> {
        nll_todo()
        // warn!("Making vote token");
        // if let Some(votes) = self.stake_table.get(private_key) {
        //     // Get the votes for our self
        //     let selection_threshold = self.calculate_selection_threshold();
        //     let hashes = self.vote_hashes(private_key, view_number, *votes, selection_threshold);
        //     if hashes.is_empty() {
        //         None
        //     } else {
        //         Some((*private_key, *votes))
        //     }
        // } else {
        //     None
        // }
    }

    fn validate_vote_token(
        &self,
        view_number: ViewNumber,
        pub_key: HashVrfKey,
        token: Checked<Self::VoteTokenType>,
        next_state: commit::Commitment<hotshot_types::data::Leaf<Self::StateType>>,
    ) -> Result<hotshot_types::traits::election::Checked<Self::VoteTokenType>, ElectionError> {
        nll_todo()
    }

    // TODO fix
    type ElectionConfigType = StaticElectionConfig;

    fn default_election_config(num_nodes: u64) -> Self::ElectionConfigType {
        todo!()
    }

    fn create_election(keys: Vec<HashVrfKey>, config: Self::ElectionConfigType) -> Self {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demos::dentry::random_leaf;
    use commit::Committable;
    use hotshot_types::traits::block_contents::dummy::{DummyBlock, DummyState};

    // Make sure the selection threshold is calculated properly
    #[test]
    fn selection_threshold() {
        // // Setup a dummy implementation
        // let key = HashVrfKey::random();
        // let seed = HashVrfSeed::random();
        // let vrf = HashElection::<DummyState>::new(
        //     [(key, 1)],
        //     seed,
        //     NonZeroU64::new(1).unwrap(),
        //     NonZeroU64::new(10).unwrap(),
        // );
        // let next_state = random_leaf(DummyBlock::random()).commit();
        // // Our strategy here is to run 10,000 trials and make sure the number of hits is within around
        // // 10% of the expected parameter
        // let mut hits: u64 = 0;
        // for i in 0..10_000 {
        //     if let Some(token) =
        //         <HashElection<DummyState> as Election<HashVrfKey, ViewNumber>>::make_vote_token(
        //             &vrf,
        //             ViewNumber::new(i),
        //             &key,
        //             next_state,
        //         )
        //     {
        //         if let Some(validated) = <HashElection<DummyState> as Election<
        //             HashVrfKey,
        //             ViewNumber,
        //         >>::get_votes(
        //             &vrf, ViewNumber::new(i), key, token, next_state
        //         ) {
        //             hits += <HashElection<DummyState> as Election<HashVrfKey, ViewNumber>>::get_vote_count(
        //                 &vrf, &validated,
        //             );
        //         }
        //     }
        // }
        // println!("{}", hits);
        // assert!((900..=1_100).contains(&hits));
    }
}
