//! Data Availability wrapper types

use std::collections::{BTreeSet, HashSet};

use commit::{Commitment, Committable, RawCommitmentBuilder};
use hotshot_types::{
    data::{random_commitment, ViewNumber},
    traits::{
        block_contents::{BlockCommitment, Transaction},
        state::{ConsensusTime, TestableBlock, TestableState},
        Block, State,
    },
};
use nll::nll_todo::nll_todo;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use snafu::Snafu;
use tracing::error;

// /// State of the data availability store, this is an unordered collection of blocks
// #[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
// #[serde(bound(deserialize = ""))]
// pub struct DAState<T: Block> {
//     /// The contained blocks
//     blocks: BTreeSet<BlockCommitment<T>>,
// }

// impl<T: Block> Default for DAState<T> {
//     fn default() -> Self {
//         Self {
//             blocks: BTreeSet::new(),
//         }
//     }
// }

// impl<T: Block> Committable for DAState<T> {
//     fn commit(&self) -> commit::Commitment<Self> {
//         let mut builder = RawCommitmentBuilder::new("DAState Commitment");
//         for block in &self.blocks {
//             builder = builder.var_size_bytes(block.0.as_ref());
//         }
//         builder.finalize()
//     }
// }

/// Transaction type for DA
// #[derive(Clone, Copy, Hash, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
// #[serde(bound(deserialize = ""))]
// pub struct DATransaction<T: Block> {
//     /// The commitment to add
//     commitment: BlockCommitment<T>,
// }

// impl<T: Block> Committable for DATransaction<T> {
//     fn commit(&self) -> commit::Commitment<Self> {
//         RawCommitmentBuilder::new("DATransaction")
//             .field("block_commitment", self.commitment.0)
//             .finalize()
//     }
// }

// impl<T: Block> Transaction for DATransaction<T> {}

// /// Block type for DA
// #[derive(
//     Default, Clone, Copy, Hash, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
// )]
// #[serde(bound(deserialize = ""))]
// pub struct DABlock<T: Block> {
//     /// Possible new commitment to add
//     transaction: Option<DATransaction<T>>,
// }

// impl<T: Block> Committable for DABlock<T> {
//     fn commit(&self) -> commit::Commitment<Self> {
//         let mut builder = RawCommitmentBuilder::new("DABlock");
//         if let Some(transaction) = &self.transaction {
//             builder = builder.field("block_commitment", transaction.commitment.0);
//         }
//         builder.finalize()
//     }
// }
//
// impl<T: Block> Block for DABlock<T> {
//     type Error = DAError;
//
//     type Transaction = DATransaction<T>;
//
//     fn add_transaction_raw(
//         &self,
//         tx: &Self::Transaction,
//     ) -> std::result::Result<Self, Self::Error> {
//         if let Some(transaction) = &self.transaction {
//             if transaction == tx {
//                 Ok(self.clone())
//             } else {
//                 AlreadyContainsCommitmentSnafu.fail()
//             }
//         } else {
//             Ok(Self {
//                 transaction: Some(tx.clone()),
//             })
//         }
//     }
//
//     // TODO set to vec
//     fn contained_transactions(&self) -> HashSet<commit::Commitment<Self::Transaction>> {
//         if let Some(transaction) = &self.transaction {
//             vec![transaction.commit()].into_iter().collect()
//         } else {
//             HashSet::new()
//         }
//     }
//
//     fn block_commit(&self) -> BlockCommitment<Self> {
//         BlockCommitment(self.commit())
//     }
// }
//
// /// Error type for DA
// #[derive(Debug, Snafu)]
// pub enum DAError {
//     /// Already contains a commitment
//     AlreadyContainsCommitment,
//     /// Block does not contain a commitment
//     NoCommitment,
// }
//
// impl<T: Block> State for DAState<T> {
//     type Error = DAError;
//
//     type BlockType = DABlock<T>;
//
//     type Time = ViewNumber;
//
//     fn next_block(&self) -> Self::BlockType {
//         DABlock { transaction: None }
//     }
//
//     fn validate_block(&self, block: &Self::BlockType, _view_number: &Self::Time) -> bool {
//         if let Some(transaction) = &block.transaction {
//             !self.blocks.contains(&transaction.commitment)
//         } else {
//             false
//         }
//     }
//
//     fn append(
//         &self,
//         block: &Self::BlockType,
//         view_number: &Self::Time,
//     ) -> Result<Self, Self::Error> {
//         if let Some(transaction) = &block.transaction {
//             let mut blocks = self.blocks.clone();
//             blocks.insert(transaction.commitment.clone());
//             Ok(Self { blocks })
//         } else {
//             // FIXME (nm/da-sprint-1): For some reason our testing harness is not generating any
//             // transactions in the first round, so we allow the block to be empty in that case as a
//             // work around. For a proper solution we need to figure out what's going on in the
//             // testing harness instead
//             if view_number <= &ViewNumber::new(1) {
//                 Ok(self.clone())
//             } else {
//                 NoCommitmentSnafu.fail()
//             }
//         }
//     }
//
//     fn on_commit(&self) {}
// }

// impl<T: Block + TestableBlock> TestableBlock for DABlock<T> {
//     fn genesis() -> Self {
//         let inner_genesis = T::genesis();
//         let commitment = BlockCommitment(inner_genesis.commit());
//         Self {
//             transaction: Some(DATransaction { commitment }),
//         }
//     }
// }
//
// impl<T: Block + TestableBlock> TestableState for DAState<T> {
//     fn create_random_transaction(
//         &self,
//         rng: &mut dyn rand::RngCore,
//     ) -> <Self::BlockType as Block>::Transaction {
//         let commitment: Commitment<T> = random_commitment(rng);
//         DATransaction {
//             commitment: BlockCommitment(commitment),
//         }
//     }
// }
