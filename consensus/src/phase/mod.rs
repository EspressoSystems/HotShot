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
use crate::{ConsensusApi, Result, RoundTimedoutState, TransactionState, ViewNumber};
use hotshot_types::{
    data::Stage,
    error::HotShotError,
    traits::node_implementation::{NodeImplementation, TypeMap},
};
use std::future::Future;
use tracing::{debug, info, instrument, warn};
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

    /// Determines the livelyness state of this viewstate.
    alive_state: ViewAliveState,

    /// The prepare phase. This will always be present
    prepare: PreparePhase<N>,

    /// The precommit phase
    precommit: Option<PreCommitPhase<I, N>>,

    /// The commit phase
    commit: Option<CommitPhase<N>>,

    /// The decide phase
    decide: Option<DecidePhase<N>>,
}

/// Determines the livelyness state of a [`ViewState`].
#[derive(Debug)]
enum ViewAliveState {
    /// The viewstate is running
    Running,
    /// The viewstate finished successfully
    Finished,
    /// The viewstate got interrupted. The inner state contains information what state the round was in.
    Interrupted(RoundTimedoutState),
}

impl ViewAliveState {
    /// Returns `true` is this state is either `Interrupted` or `Finished`.
    fn is_done(&self) -> bool {
        matches!(
            self,
            ViewAliveState::Interrupted(_) | ViewAliveState::Finished
        )
    }
}

impl<I: NodeImplementation<N>, const N: usize> ViewState<I, N> {
    /// Create a new `prepare` phase with the given `view_number`.
    pub fn prepare(view_number: ViewNumber, is_leader: bool) -> Self {
        Self {
            view_number,
            messages: Vec::new(),
            alive_state: ViewAliveState::Running,

            prepare: PreparePhase::new(is_leader),
            precommit: None,
            commit: None,
            decide: None,
        }
    }

    /// Returns the current stage of this phase
    pub fn stage(&self) -> Stage {
        if self.alive_state.is_done() {
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
        debug!(?message, "Incoming message");
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
        debug!("New transactions available");
        self.update(api, transactions).await
    }

    /// Update the current state with the given transactions.
    ///
    /// # Errors
    ///
    /// Will return an error when a stage is invalid, or when any of the [`ConsensusApi`] methods return an error.
    #[instrument(skip(api))]
    async fn update<A: ConsensusApi<I, N>>(
        &mut self,
        api: &mut A,
        transactions: &mut [TransactionState<I, N>],
    ) -> Result {
        if self.alive_state.is_done() {
            warn!(?self, "Phase is done, no updates will be run");
            return Ok(());
        }
        // This loop will make sure that when a stage transition happens, the next stage will execute immediately
        loop {
            let is_leader = api.is_leader(self.view_number, self.stage()).await;
            let stage = self.stage();
            let mut ctx = UpdateCtx {
                is_leader,
                api,
                messages: &mut self.messages,
                view_number: self.view_number,
                stage,
            };
            match stage {
                Stage::None => unreachable!(),
                Stage::Prepare => {
                    if let Progress::Next(precommit) =
                        self.prepare.update(&mut ctx, transactions).await?
                    {
                        debug!(?precommit, "Transitioning from prepare to precommit");
                        self.precommit = Some(precommit);
                    } else {
                        break Ok(());
                    }
                }
                Stage::PreCommit => {
                    if let Some(commit) =
                        update(&mut self.precommit, PreCommitPhase::update, &mut ctx).await?
                    {
                        debug!(?commit, "Transitioning from precommit to commit");
                        self.commit = Some(commit);
                    } else {
                        break Ok(());
                    }
                }
                Stage::Commit => {
                    if let Some(decide) =
                        update(&mut self.commit, CommitPhase::update, &mut ctx).await?
                    {
                        debug!(?decide, "Transitioning from commit to decide");
                        self.decide = Some(decide);
                    } else {
                        break Ok(());
                    }
                }
                Stage::Decide => {
                    if let Some(()) =
                        update(&mut self.decide, DecidePhase::update, &mut ctx).await?
                    {
                        self.alive_state = ViewAliveState::Finished;
                        info!(?self, "Phase completed");
                    }
                    break Ok(());
                }
            }
        }
    }

    /// Return true if this phase was finished. Query `was_timed_out` to determine if this was successfull
    pub fn is_done(&self) -> bool {
        self.alive_state.is_done()
    }

    /// Return true if this phase has run until completion
    pub fn get_timedout_reason(&self) -> Option<RoundTimedoutState> {
        match &self.alive_state {
            ViewAliveState::Interrupted(reason) => Some(reason.clone()),
            _ => None,
        }
    }

    /// Called when the round is timed out. May do some cleanup logic.
    pub fn timeout(&mut self) {
        self.alive_state = ViewAliveState::Interrupted(if let Some(decide) = &self.decide {
            decide.timeout_reason()
        } else if let Some(commit) = &self.commit {
            commit.timeout_reason()
        } else if let Some(precommit) = &self.precommit {
            precommit.timeout_reason()
        } else {
            self.prepare.timeout_reason()
        });
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
        Err(HotShotError::InvalidState {
            context: format!(
                "Could not update phase {:?}, it is None",
                std::any::type_name::<PHASE>()
            ),
        })
    }
}
