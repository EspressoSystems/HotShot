// TODO(vko): Merge collect_transactions and prepare

mod collect_transactions;
mod commit;
mod precommit;
mod prepare;

use collect_transactions::{CollectTransactions, CollectTransactionsTransition};
use commit::{Commit, CommitTransition};
use precommit::{PreCommit, PreCommitTransition};
use prepare::{Prepare, PrepareTransition};

use crate::{ConsensusApi, ConsensusMessage, Result};
use phaselock_types::{data::Leaf, traits::node_implementation::NodeImplementation};
use std::fmt::Debug;
use tracing::instrument;

#[derive(Debug, Clone)]
pub enum LeaderPhase<I: NodeImplementation<N>, const N: usize> {
    Prepare(Prepare<N>),
    CollectTransactions(CollectTransactions<I, N>),
    PreCommit(PreCommit<I, N>),
    Commit(Commit<I, N>),
}

pub enum LeaderTransition {
    /// Stay in this state, do not transition.
    Stay,

    /// Round has ended
    End { current_view: u64 },
}

impl<I: NodeImplementation<N>, const N: usize> LeaderPhase<I, N> {
    pub(super) fn new(view_number: u64) -> Self {
        Self::Prepare(Prepare::new(view_number))
    }

    #[instrument(skip(api))]
    pub(super) async fn step(
        &mut self,
        msg: ConsensusMessage<'_, I, N>,
        api: &mut impl ConsensusApi<I, N>,
    ) -> Result<LeaderTransition> {
        match self {
            Self::Prepare(prepare) => match prepare.step(msg, api).await? {
                PrepareTransition::Stay => Ok(LeaderTransition::Stay),
                PrepareTransition::CollectTransactions { ctx, high_qc } => {
                    *self =
                        LeaderPhase::CollectTransactions(CollectTransactions::new(ctx, high_qc));
                    Ok(LeaderTransition::Stay)
                }
            },

            Self::CollectTransactions(collect_transactions) => {
                match collect_transactions.step(msg, api).await? {
                    CollectTransactionsTransition::Stay => Ok(LeaderTransition::Stay),
                    CollectTransactionsTransition::PreCommit { ctx, vote } => {
                        *self = LeaderPhase::PreCommit(PreCommit::new(ctx, vote));
                        Ok(LeaderTransition::Stay)
                    }
                }
            }

            Self::PreCommit(precommit) => match precommit.step(msg, api).await? {
                PreCommitTransition::Stay => Ok(LeaderTransition::Stay),
                PreCommitTransition::Commit { ctx, qc, vote } => {
                    *self = LeaderPhase::Commit(Commit::new(ctx, qc, vote));
                    Ok(LeaderTransition::Stay)
                }
            },

            Self::Commit(commit) => match commit.step(msg, api).await? {
                CommitTransition::Stay => Ok(LeaderTransition::Stay),
                CommitTransition::End { current_view } => {
                    Ok(LeaderTransition::End { current_view })
                }
            },
        }
    }
}

/// Common values that get used in a lot of phases
#[derive(Debug, Clone)]
#[allow(dead_code)] // TODO(vko): Prune unused variables
struct Ctx<I: NodeImplementation<N>, const N: usize> {
    pub current_view: u64,

    pub block: I::Block,
    pub state: I::State,
    pub leaf: Leaf<I::Block, N>,

    pub committed_state: I::State,
    pub committed_leaf: Leaf<I::Block, N>,
}
