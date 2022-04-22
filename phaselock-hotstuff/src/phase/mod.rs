//! Contains the 4 phases that hotstuff defines:
//! - [`PreparePhase`]
//! - [`PreCommitPhase`]
//! - [`CommitPhase`]
//! - [`DecidePhase`]

mod commit;
mod decide;
mod precommit;
mod prepare;
mod update_ctx;

use self::{
    commit::CommitPhase, decide::DecidePhase, precommit::PreCommitPhase, prepare::PreparePhase,
};
use crate::{ConsensusApi, Result, TransactionState, ViewNumber};
use phaselock_types::{
    data::Stage,
    error::PhaseLockError,
    traits::node_implementation::{NodeImplementation, TypeMap},
};
use std::future::Future;
use tracing::{info, trace};
use update_ctx::UpdateCtx;

/// Contains all the information about a current `view_number`.
///
/// Internally this has 4 stages:
/// - [`PreparePhase`]
/// - [`PreCommitPhase`]
/// - [`CommitPhase`]
/// - [`DecidePhase`]
///
/// For more info about these phases, see the hotstuff paper.
///
/// Whenever a phase is done, this will progress to a next phase. When `decide` is done an internal `done` boolean is set to `true`.
#[derive(Debug)]
pub(crate) struct ViewState<I: NodeImplementation<N>, const N: usize> {
    /// The view number of this phase.
    view_number: ViewNumber,

    /// All messages that have been received on this phase.
    /// In the future these could be trimmed whenever messages are being used, but for now they are stored in memory for debugging purposes.
    messages: Vec<<I as TypeMap<N>>::ConsensusMessage>,

    /// if `true` this phase is done and will not run any more updates
    done: bool,

    /// The prepare phase. This will always be present
    prepare: PreparePhase<N>,

    /// The precommit phase
    precommit: Option<PreCommitPhase<I, N>>,

    /// The commit phase
    commit: Option<CommitPhase<N>>,

    /// The decide phase
    decide: Option<DecidePhase<N>>,
}

impl<I: NodeImplementation<N>, const N: usize> ViewState<I, N> {
    /// Create a new `prepare` phase with the given `view_number`.
    pub fn prepare(view_number: ViewNumber, is_leader: bool) -> Self {
        Self {
            view_number,
            messages: Vec::new(),
            done: false,

            prepare: PreparePhase::new(is_leader),
            precommit: None,
            commit: None,
            decide: None,
        }
    }

    /// Returns the current stage of this phase
    pub fn stage(&self) -> Stage {
        if self.done {
            Stage::None
        } else if self.decide.is_some() {
            Stage::Decide
        } else if self.commit.is_some() {
            Stage::Commit
        } else if self.precommit.is_some() {
            Stage::PreCommit
        } else {
            Stage::Prepare
        }
    }

    /// Notify this phase that a new message has been received
    ///
    /// # Errors
    ///
    /// Will return an error when a stage is invalid, or when any of the [`ConsensusApi`] methods return an error.
    pub async fn add_consensus_message<A: ConsensusApi<I, N>>(
        &mut self,
        api: &mut A,
        transactions: &mut [TransactionState<I, N>],
        message: <I as TypeMap<N>>::ConsensusMessage,
    ) -> Result {
        self.messages.push(message.clone());
        self.update(api, transactions).await
    }

    /// Notify this phase that new transactions are available.
    ///
    /// # Errors
    ///
    /// Will return an error when a stage is invalid, or when any of the [`ConsensusApi`] methods return an error.
    pub async fn notify_new_transaction<A: ConsensusApi<I, N>>(
        &mut self,
        api: &mut A,
        transactions: &mut [TransactionState<I, N>],
    ) -> Result {
        self.update(api, transactions).await
    }

    /// Update the current state with the given transactions.
    ///
    /// # Errors
    ///
    /// Will return an error when a stage is invalid, or when any of the [`ConsensusApi`] methods return an error.
    async fn update<A: ConsensusApi<I, N>>(
        &mut self,
        api: &mut A,
        transactions: &mut [TransactionState<I, N>],
    ) -> Result {
        if self.done {
            trace!(?self, "Phase is done, no updates will be run");
            return Ok(());
        }
        let is_leader = api.is_leader(self.view_number.0, self.stage()).await;
        let stage = self.stage();
        let mut ctx = UpdateCtx {
            is_leader,
            api,
            messages: &mut self.messages,
            transactions,
            view_number: self.view_number,
            stage,
        };

        match stage {
            Stage::None => {
                unreachable!()
            }
            Stage::Prepare => {
                if let Progress::Next(precommit) = self.prepare.update(&mut ctx).await? {
                    self.precommit = Some(precommit);
                }
                Ok(())
            }
            Stage::PreCommit => {
                if let Some(commit) =
                    update(&mut self.precommit, PreCommitPhase::update, &mut ctx).await?
                {
                    self.commit = Some(commit);
                }
                Ok(())
            }
            Stage::Commit => {
                if let Some(decide) =
                    update(&mut self.commit, CommitPhase::update, &mut ctx).await?
                {
                    self.decide = Some(decide);
                }
                Ok(())
            }
            Stage::Decide => {
                if let Some(()) = update(&mut self.decide, DecidePhase::update, &mut ctx).await? {
                    self.done = true;
                    info!(?self, "Phase completed");
                }
                Ok(())
            }
        }
    }

    /// Return true if this phase has run until completion
    pub fn is_done(&self) -> bool {
        self.done
    }
}

/// Determines if a stage is ready to progress to the next stage
enum Progress<T> {
    /// This stage is not ready to progress
    NotReady,
    /// This stage is ready to progress to the next stage
    Next(T),
}

/// Update a given `Option<PHASE>`, calling `FN` with the value and `ctx` if it is not None.
///
/// If `FN` returns `Progress::Next(RET)` this function will return `Some(RET)`.
///
/// # Errors
///
/// Will return an error if:
/// - `Option<PHASE>` is `None`
/// - `FN` returns an error
async fn update<'a, I, A, RET, PHASE, FN, FUT, const N: usize>(
    t: &'a mut Option<PHASE>,
    f: FN,
    ctx: &'a mut UpdateCtx<'a, I, A, N>,
) -> Result<Option<RET>>
where
    I: NodeImplementation<N>,
    A: ConsensusApi<I, N>,
    FN: FnOnce(&'a mut PHASE, &'a mut UpdateCtx<'a, I, A, N>) -> FUT + 'a,
    FUT: Future<Output = Result<Progress<RET>>> + 'a,
{
    if let Some(phase) = t.as_mut() {
        if let Progress::Next(ret) = f(phase, ctx).await? {
            Ok(Some(ret))
        } else {
            Ok(None)
        }
    } else {
        Err(PhaseLockError::InvalidState {
            context: format!(
                "Could not update phase {:?}, it is None",
                std::any::type_name::<PHASE>()
            ),
        })
    }
}
