mod prepare;
mod propose;

use std::future::Future;

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
use prepare::PreparePhase;
use propose::ProposePhase;
use snafu::ResultExt;

pub(crate) struct Phase<I: NodeImplementation<N>, const N: usize> {
    view_number: ViewNumber,
    messages: Vec<<I as TypeMap<N>>::ConsensusMessage>,
    propose: Option<ProposePhase<N>>,
    prepare: Option<PreparePhase<N>>,
}

impl<I: NodeImplementation<N>, const N: usize> Phase<I, N> {
    pub fn propose(view_number: ViewNumber) -> Self {
        Self {
            view_number,
            messages: Vec::new(),
            propose: Some(ProposePhase::new()),
            prepare: None,
        }
    }

    pub fn prepare(view_number: ViewNumber) -> Self {
        Self {
            view_number,
            messages: Vec::new(),
            propose: None,
            prepare: Some(PreparePhase::new()),
        }
    }

    pub fn stage(&self) -> Stage {
        if (self.propose.is_some() && self.prepare.is_none()) || self.prepare.is_some() {
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
            ConsensusMessage::NewView(_) => {
                if let Some(prepare) =
                    update(&mut self.propose, ProposePhase::update, &mut ctx).await?
                {
                    self.prepare = Some(prepare);
                }
                Ok(())
            }
            ConsensusMessage::Prepare(_prepare) => {
                todo!()
            }
            ConsensusMessage::PrepareVote(_) => todo!(),
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
        if let Some(prepare) = &mut self.prepare {
            if let Progress::Next(()) = prepare.update(&mut ctx).await? {
                todo!();
            }
        } else if let Some(propose) = &mut self.propose {
            if let Progress::Next(prepare) = propose.update(&mut ctx).await? {
                self.prepare = Some(prepare);
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
