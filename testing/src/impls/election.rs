use commit::Commitment;
use hotshot::{data::Leaf, traits::dummy::DummyState};
use hotshot_types::{
    data::ViewNumber,
    traits::{
        election::{Checked, Election, ElectionConfig, VoteToken},
        signature_key::ed25519::Ed25519Pub,
    },
};
use nll::nll_todo::nll_todo;
use tracing::info;

/// A testable interface for the election trait.
#[derive(Debug)]
pub struct TestElection {
    /// These leaders will be picked. If this list runs out the test will panic.
    pub leaders: Vec<Ed25519Pub>,
}

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash)]
pub struct StubToken {}

impl VoteToken for StubToken {
    fn vote_count(&self) -> u64 {
        nll_todo()
    }
}

impl Election<Ed25519Pub, ViewNumber> for TestElection {
    type StakeTable = ();
    type StateType = DummyState;

    type VoteTokenType = StubToken;

    fn get_stake_table(
        &self,
        view_number: ViewNumber,
        state: &Self::StateType,
    ) -> Self::StakeTable {
        nll_todo()
    }

    fn get_leader(&self, view_number: ViewNumber) -> Ed25519Pub {
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

    fn make_vote_token(
        &self,
        view_number: ViewNumber,
        private_key: &<Ed25519Pub as hotshot::types::SignatureKey>::PrivateKey,
    ) -> Result<Option<Self::VoteTokenType>, hotshot_types::traits::election::ElectionError> {
        nll_todo()
    }

    fn validate_vote_token(
        &self,
        view_number: ViewNumber,
        pub_key: Ed25519Pub,
        token: Checked<Self::VoteTokenType>,
    ) -> Result<
        hotshot_types::traits::election::Checked<Self::VoteTokenType>,
        hotshot_types::traits::election::ElectionError,
    > {
        nll_todo()
    }

    type ElectionConfigType = ElectionConfigStub;

    fn default_election_config(num_nodes: u64) -> Self::ElectionConfigType {
        ElectionConfigStub {}
    }

    fn create_election(keys: Vec<Ed25519Pub>, config: Self::ElectionConfigType) -> Self {
        todo!()
    }
}

#[derive(Default, Clone, Serialize, Deserialize, core::fmt::Debug)]
pub struct ElectionConfigStub {}

impl ElectionConfig for ElectionConfigStub {}
