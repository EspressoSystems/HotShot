use phaselock::{data::StateHash, PubKey};
use phaselock_types::{
    data::{Stage, ViewNumber},
    traits::{election::Election, signature_key::SignatureKey},
};
use tracing::{info, instrument};

/// A testable interface for the election trait.
#[derive(Debug)]
pub struct TestElection<K: SignatureKey> {
    /// These leaders will be picked. If this list runs out the test will panic.
    pub leaders: Vec<PubKey<K>>,
}

impl<K: SignatureKey, const N: usize> Election<K, N> for TestElection<K> {
    type StakeTable = ();
    type SelectionThreshold = ();
    type State = ();
    type VoteToken = ();
    type ValidatedVoteToken = ();

    fn get_stake_table(&self, _: &Self::State) -> Self::StakeTable {}

    fn get_leader(&self, _: &Self::StakeTable, view_number: ViewNumber, _: Stage) -> PubKey<K> {
        match self.leaders.get(*view_number as usize) {
            Some(leader) => {
                info!("Round {:?} has leader {:?}", view_number, leader);
                leader.clone()
            }
            None => {
                panic!("Round {:?} has no leader", view_number);
            }
        }
    }

    #[instrument]
    fn get_votes(
        &self,
        table: &Self::StakeTable,
        selection_threshold: Self::SelectionThreshold,
        view_number: ViewNumber,
        pub_key: PubKey<K>,
        token: Self::VoteToken,
        next_state: StateHash<N>,
    ) -> Option<Self::ValidatedVoteToken> {
        None
    }

    #[instrument]
    fn get_vote_count(&self, token: &Self::ValidatedVoteToken) -> u64 {
        todo!()
    }

    #[instrument(skip(_private_key))]
    fn make_vote_token(
        &self,
        table: &Self::StakeTable,
        selection_threshold: Self::SelectionThreshold,
        view_number: ViewNumber,
        _private_key: &K::PrivateKey,
        next_state: phaselock::data::StateHash<N>,
    ) -> Option<Self::VoteToken> {
        todo!()
    }
}
