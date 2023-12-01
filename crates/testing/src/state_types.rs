//! Implementations for examples and tests only
use commit::{Commitment, Committable};

use hotshot_types::{
    data::{fake_commitment, BlockError, ViewNumber},
    traits::{
        state::{ConsensusTime, TestableState},
        BlockPayload, State,
    },
};

use serde::{Deserialize, Serialize};
use std::fmt::Debug;

use crate::block_types::TestTransaction;
use crate::block_types::{TestBlockHeader, TestBlockPayload};
pub use crate::node_types::TestTypes;

/// sequencing demo entry state
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct TestState {
    /// the block height
    block_height: u64,
    /// the view number
    view_number: ViewNumber,
    /// the previous state commitment
    prev_state_commitment: Commitment<Self>,
}

impl Committable for TestState {
    fn commit(&self) -> Commitment<Self> {
        commit::RawCommitmentBuilder::new("Test State Commit")
            .u64_field("block_height", self.block_height)
            .u64_field("view_number", *self.view_number)
            .field("prev_state_commitment", self.prev_state_commitment)
            .finalize()
    }

    fn tag() -> String {
        "SEQUENCING_TEST_STATE".to_string()
    }
}

impl Default for TestState {
    fn default() -> Self {
        Self {
            block_height: 0,
            view_number: ViewNumber::genesis(),
            prev_state_commitment: fake_commitment(),
        }
    }
}

impl State for TestState {
    type Error = BlockError;

    type BlockHeader = TestBlockHeader;

    type BlockPayload = TestBlockPayload;

    type Time = ViewNumber;

    fn validate_block(&self, _block_header: &Self::BlockHeader, view_number: &Self::Time) -> bool {
        if view_number == &ViewNumber::genesis() {
            &self.view_number == view_number
        } else {
            self.view_number < *view_number
        }
    }

    fn initialize() -> Self {
        let mut state = Self::default();
        state.block_height += 1;
        state
    }

    fn append(
        &self,
        block_header: &Self::BlockHeader,
        view_number: &Self::Time,
    ) -> Result<Self, Self::Error> {
        if !self.validate_block(block_header, view_number) {
            return Err(BlockError::InvalidBlockHeader);
        }

        Ok(TestState {
            block_height: self.block_height + 1,
            view_number: *view_number,
            prev_state_commitment: self.commit(),
        })
    }

    fn on_commit(&self) {}
}

impl TestableState for TestState {
    fn create_random_transaction(
        _state: Option<&Self>,
        _rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <Self::BlockPayload as BlockPayload>::Transaction {
        /// clippy appeasement for `RANDOM_TX_BASE_SIZE`
        const RANDOM_TX_BASE_SIZE: usize = 8;
        TestTransaction(vec![0; RANDOM_TX_BASE_SIZE + (padding as usize)])
    }
}
