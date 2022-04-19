use crate::{ConsensusApi, OptionUtils, Result, TransactionState, ViewNumber};
use phaselock_types::{
    data::{BlockHash, Leaf, LeafHash, QuorumCertificate},
    error::StorageSnafu,
    message::{Commit, ConsensusMessage, Decide, NewView, PreCommit, Prepare, Vote},
    traits::{
        node_implementation::{NodeImplementation, TypeMap},
        storage::Storage,
    },
};
use snafu::ResultExt;

pub(super) struct UpdateCtx<'a, I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize> {
    pub(super) api: &'a mut A,
    pub(super) view_number: ViewNumber,
    pub(super) transactions: &'a mut [TransactionState<I, N>],
    pub(super) messages: &'a [<I as TypeMap<N>>::ConsensusMessage],
    pub(super) is_leader: bool,
}

impl<'a, I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize> UpdateCtx<'a, I, A, N> {
    pub(super) async fn get_leaf_by_block(
        &self,
        block: &BlockHash<N>,
    ) -> Result<Leaf<I::Block, N>> {
        self.api
            .storage()
            .get_leaf_by_block(block)
            .await
            .context(StorageSnafu)?
            .or_not_found(block)
    }

    pub(super) async fn get_state_by_leaf(&self, leaf: &LeafHash<N>) -> Result<I::State> {
        self.api
            .storage()
            .get_state(leaf)
            .await
            .context(StorageSnafu)?
            .or_not_found(leaf)
    }

    pub(super) async fn get_newest_qc(&self) -> Result<Option<QuorumCertificate<N>>> {
        self.api
            .storage()
            .get_newest_qc()
            .await
            .context(StorageSnafu)
    }

    fn messages<'this, FN, RET>(&'this self, filter: FN) -> impl Iterator<Item = RET> + 'this
    where
        FN: FnMut(&'this <I as TypeMap<N>>::ConsensusMessage) -> Option<RET> + 'this,
    {
        self.messages.iter().filter_map(filter)
    }

    pub(super) fn new_view_messages(&self) -> impl Iterator<Item = &NewView<N>> + '_ {
        self.messages(|m| {
            if let ConsensusMessage::NewView(nv) = m {
                Some(nv)
            } else {
                None
            }
        })
    }

    pub(super) fn prepare_message(&self) -> Option<&Prepare<I::Block, I::State, N>> {
        self.messages(|m| {
            if let ConsensusMessage::Prepare(prepare) = m {
                Some(prepare)
            } else {
                None
            }
        })
        .last()
    }

    pub(super) fn prepare_vote_messages(&self) -> impl Iterator<Item = &Vote<N>> + '_ {
        self.messages(|m| {
            if let ConsensusMessage::PrepareVote(prepare) = m {
                Some(prepare)
            } else {
                None
            }
        })
    }

    #[allow(dead_code)] // TODO(vko): cleanup
    pub(super) fn pre_commit_message(&self) -> Option<&PreCommit<N>> {
        self.messages(|m| {
            if let ConsensusMessage::PreCommit(prepare) = m {
                Some(prepare)
            } else {
                None
            }
        })
        .last()
    }

    #[allow(dead_code)] // TODO(vko): cleanup
    pub(super) fn pre_commit_vote_messages(&self) -> impl Iterator<Item = &Vote<N>> + '_ {
        self.messages(|m| {
            if let ConsensusMessage::PreCommitVote(prepare) = m {
                Some(prepare)
            } else {
                None
            }
        })
    }

    #[allow(dead_code)] // TODO(vko): cleanup
    pub(super) fn commit_message(&self) -> Option<&Commit<N>> {
        self.messages(|m| {
            if let ConsensusMessage::Commit(prepare) = m {
                Some(prepare)
            } else {
                None
            }
        })
        .last()
    }

    #[allow(dead_code)] // TODO(vko): cleanup
    pub(super) fn commit_vote_messages(&self) -> impl Iterator<Item = &Vote<N>> + '_ {
        self.messages(|m| {
            if let ConsensusMessage::CommitVote(prepare) = m {
                Some(prepare)
            } else {
                None
            }
        })
    }

    #[allow(dead_code)] // TODO(vko): cleanup
    pub(super) fn decide_message(&self) -> Option<&Decide<N>> {
        self.messages(|m| {
            if let ConsensusMessage::Decide(prepare) = m {
                Some(prepare)
            } else {
                None
            }
        })
        .last()
    }

    pub(super) fn get_unclaimed_transactions_mut(
        &mut self,
    ) -> impl Iterator<Item = &mut TransactionState<I, N>> + '_ {
        self.transactions.iter_mut().filter(|t| t.is_unclaimed())
    }
}
