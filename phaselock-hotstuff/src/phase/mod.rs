mod precommit;
mod prepare;
mod update_ctx;

use crate::{ConsensusApi, Result, TransactionState, ViewNumber};
use phaselock_types::{
    data::Stage,
    error::PhaseLockError,
    traits::node_implementation::{NodeImplementation, TypeMap},
};
use precommit::PreCommitPhase;
use prepare::PreparePhase;
use std::future::Future;
use tracing::warn;
use update_ctx::UpdateCtx;

#[derive(Debug)]
pub(crate) struct Phase<I: NodeImplementation<N>, const N: usize> {
    view_number: ViewNumber,
    messages: Vec<<I as TypeMap<N>>::ConsensusMessage>,
    prepare: Option<PreparePhase<N>>,
    precommit: Option<PreCommitPhase<N>>,
}

impl<I: NodeImplementation<N>, const N: usize> Phase<I, N> {
    pub fn prepare(view_number: ViewNumber, is_leader: bool) -> Self {
        Self {
            view_number,
            messages: Vec::new(),
            prepare: Some(if is_leader {
                PreparePhase::leader()
            } else {
                PreparePhase::replica()
            }),
            precommit: None,
        }
    }

    pub fn stage(&self) -> Stage {
        if self.precommit.is_some() {
            Stage::PreCommit
        } else if self.prepare.is_some() {
            Stage::Prepare
        } else {
            Stage::None
        }
    }

    pub async fn add_consensus_message<A: ConsensusApi<I, N>>(
        &mut self,
        api: &mut A,
        transactions: &mut [TransactionState<I, N>],
        message: <I as TypeMap<N>>::ConsensusMessage,
    ) -> Result {
        self.messages.push(message.clone());
        self.update(api, transactions).await
    }

    pub async fn notify_new_transaction<A: ConsensusApi<I, N>>(
        &mut self,
        api: &mut A,
        transactions: &mut [TransactionState<I, N>],
    ) -> Result {
        self.update(api, transactions).await
    }

    async fn update<A: ConsensusApi<I, N>>(
        &mut self,
        api: &mut A,
        transactions: &mut [TransactionState<I, N>],
    ) -> Result {
        let is_leader = api.is_leader(self.view_number.0, self.stage()).await;
        let mut ctx = UpdateCtx {
            is_leader,
            api,
            messages: &self.messages,
            transactions,
            view_number: self.view_number,
        };

        match self.stage() {
            Stage::None => {
                warn!(?self, "Phase is in stage mode");
                Ok(())
            }
            Stage::Prepare => {
                if let Some(precommit) =
                    update(&mut self.prepare, PreparePhase::update, &mut ctx).await?
                {
                    self.precommit = Some(precommit);
                }
                Ok(())
            }
            Stage::PreCommit => {
                if let Some(()) =
                    update(&mut self.precommit, PreCommitPhase::update, &mut ctx).await?
                {
                    todo!()
                }
                Ok(())
            }
            Stage::Commit | Stage::Decide => todo!(),
        }
    }
}
enum Progress<T> {
    NotReady,
    Next(T),
}

pub(self) fn err<T, S>(e: S) -> Result<T>
where
    S: Into<String>,
{
    Err(PhaseLockError::InvalidState { context: e.into() })
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
