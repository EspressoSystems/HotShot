use commit::Commitment;
use hotshot::{data::Leaf, traits::dummy::DummyState};
use hotshot_types::{
    data::ViewNumber,
    traits::{
        election::{Checked, ElectionConfig, VoteToken},
        signature_key::ed25519::Ed25519Pub,
    },
};
#[allow(deprecated)]use nll::nll_todo::nll_todo;
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
        println!("here vote_count");
        nll_todo()
    }
}

#[derive(Default, Clone, Serialize, Deserialize, core::fmt::Debug)]
pub struct ElectionConfigStub {}

impl ElectionConfig for ElectionConfigStub {}
