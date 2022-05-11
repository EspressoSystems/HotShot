use phaselock::{
    data::{Stage, StateHash},
    PubKey,
};
use phaselock_types::{data::ViewNumber, traits::election::Election};
use tracing::{info, instrument};

/// A testable interface for the election trait.
#[derive(Debug)]
pub struct TestElection {
    /// These leaders will be picked. If this list runs out the test will panic.
    pub leaders: Vec<PubKey>,
}

impl<const N: usize> Election<N> for TestElection {
    type StakeTable = ();
    type SelectionThreshold = ();
    type State = ();
    type VoteToken = ();
    type ValidatedVoteToken = ();

    fn get_stake_table(&self, _: &Self::State) -> Self::StakeTable {}

    fn get_leader(&self, _: &Self::StakeTable, view_number: ViewNumber, _: Stage) -> PubKey {
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
        pub_key: PubKey,
        token: Self::VoteToken,
        next_state: StateHash<N>,
    ) -> Option<Self::ValidatedVoteToken> {
        None
    }

    #[instrument]
    fn get_vote_count(&self, token: &Self::ValidatedVoteToken) -> u64 {
        todo!()
    }

    #[instrument]
    fn make_vote_token(
        &self,
        table: &Self::StakeTable,
        selection_threshold: Self::SelectionThreshold,
        view_number: ViewNumber,
        private_key: &phaselock::PrivKey,
        next_state: phaselock::data::StateHash<N>,
    ) -> Option<Self::VoteToken> {
        todo!()
    }
}
