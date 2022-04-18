mod precommit;
mod prepare;

use crate::{ConsensusApi, OptionUtils, Result, TransactionState, ViewNumber};
use phaselock_types::{
    data::{BlockHash, Leaf, LeafHash, Stage},
    error::{PhaseLockError, StorageSnafu},
    message::{ConsensusMessage, NewView},
    traits::{
        node_implementation::{NodeImplementation, TypeMap},
        storage::Storage,
    },
};
use precommit::PreCommitPhase;
use prepare::PreparePhase;
use snafu::ResultExt;
use std::future::Future;

pub(crate) struct Phase<I: NodeImplementation<N>, const N: usize> {
    view_number: ViewNumber,
    messages: Vec<<I as TypeMap<N>>::ConsensusMessage>,
    prepare: Option<PreparePhase<N>>,
    precommit: Option<PreCommitPhase>,
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
        if self.prepare.is_some() {
            // TODO: Do we want a Propose stage?
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
        let is_leader = api.is_leader_this_round(self.view_number.0).await;
        let mut ctx = UpdateCtx {
            is_leader,
            api,
            messages: &self.messages,
            transactions,
            view_number: self.view_number,
        };
        #[allow(clippy::match_same_arms)] // TODO(vko): remove
        match message {
            ConsensusMessage::NewView(_)
            | ConsensusMessage::Prepare(_)
            | ConsensusMessage::PrepareVote(_) => {
                if let Some(precommit) =
                    update(&mut self.prepare, PreparePhase::update, &mut ctx).await?
                {
                    self.precommit = Some(precommit);
                }
                Ok(())
            }
            ConsensusMessage::PreCommit(_) => todo!(),
            ConsensusMessage::PreCommitVote(_) => todo!(),
            ConsensusMessage::Commit(_) => todo!(),
            ConsensusMessage::CommitVote(_) => todo!(),
            ConsensusMessage::Decide(_) => todo!(),
        }
    }

    pub async fn notify_new_transaction<A: ConsensusApi<I, N>>(
        &mut self,
        api: &mut A,
        transactions: &mut [TransactionState<I, N>],
    ) -> Result {
        let is_leader = api.is_leader_this_round(self.view_number.0).await;
        let mut ctx = UpdateCtx {
            is_leader,
            api,
            messages: &self.messages,
            transactions,
            view_number: self.view_number,
        };
        if self.precommit.is_some() {
        } else if let Some(prepare) = &mut self.prepare {
            if let Progress::Next(precommit) = prepare.update(&mut ctx).await? {
                self.precommit = Some(precommit);
            }
        }
        todo!()
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

struct UpdateCtx<'a, I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize> {
    api: &'a mut A,
    view_number: ViewNumber,
    transactions: &'a mut [TransactionState<I, N>],
    messages: &'a [<I as TypeMap<N>>::ConsensusMessage],
    is_leader: bool,
}

impl<'a, I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize> UpdateCtx<'a, I, A, N> {
    pub async fn get_leaf_by_block(&self, block: &BlockHash<N>) -> Result<Leaf<I::Block, N>> {
        self.api
            .storage()
            .get_leaf_by_block(block)
            .await
            .context(StorageSnafu)?
            .or_not_found(block)
    }

    pub async fn get_state_by_leaf(&self, leaf: &LeafHash<N>) -> Result<I::State> {
        self.api
            .storage()
            .get_state(leaf)
            .await
            .context(StorageSnafu)?
            .or_not_found(leaf)
    }

    fn new_view_messages(&self) -> impl Iterator<Item = &NewView<N>> + '_ {
        self.messages.iter().filter_map(|m| {
            if let ConsensusMessage::NewView(nv) = m {
                Some(nv)
            } else {
                None
            }
        })
    }

    fn get_unclaimed_transactions_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut TransactionState<I, N>> + '_ {
        self.transactions.iter_mut().filter(|t| t.is_unclaimed())
    }
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
