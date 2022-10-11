use commit::Commitment;
use hotshot::traits::dummy::DummyState;
use hotshot_types::{
    data::ViewNumber,
    traits::{
        election::Election,
        signature_key::ed25519::{Ed25519Priv, Ed25519Pub},
    },
};
use tracing::{info, instrument};

/// A testable interface for the election trait.
#[derive(Debug)]
pub struct TestElection {
    /// These leaders will be picked. If this list runs out the test will panic.
    pub leaders: Vec<Ed25519Pub>,
}

impl Election<Ed25519Pub, ViewNumber> for TestElection {
    type StakeTable = ();
    type SelectionThreshold = ();
    type State = DummyState;
    type VoteToken = ();
    type ValidatedVoteToken = ();

    fn get_stake_table(&self, _: &Self::State) -> Self::StakeTable {}

    fn get_leader(&self, _: &Self::StakeTable, view_number: ViewNumber) -> Ed25519Pub {
        match self.leaders.get(*view_number as usize) {
            Some(leader) => {
                info!("Round {:?} has leader {:?}", view_number, leader);
                *leader
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
        pub_key: Ed25519Pub,
        token: Self::VoteToken,
        next_state: Commitment<Self::State>,
    ) -> Option<Self::ValidatedVoteToken> {
        Some(())
    }

    #[instrument]
    fn get_vote_count(&self, token: &Self::ValidatedVoteToken) -> u64 {
        unimplemented!()
    }

    #[instrument(skip(_private_key))]
    fn make_vote_token(
        &self,
        table: &Self::StakeTable,
        selection_threshold: Self::SelectionThreshold,
        view_number: ViewNumber,
        _private_key: &Ed25519Priv,
        next_state: Commitment<Self::State>,
    ) -> Option<Self::VoteToken> {
        unimplemented!()
    }

    fn calculate_selection_threshold(
        &self,
        _expected_size: std::num::NonZeroUsize,
        _total_participants: std::num::NonZeroUsize,
    ) -> Self::SelectionThreshold {
    }
}
