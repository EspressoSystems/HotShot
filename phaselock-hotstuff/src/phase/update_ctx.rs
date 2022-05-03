//! The update context that is used by the different phases.

use crate::{ConsensusApi, OptionUtils, Result, TransactionState, ViewNumber};
use phaselock_types::{
    data::{BlockHash, Leaf, LeafHash, QuorumCertificate, Stage},
    error::{FailedToBroadcastSnafu, StorageSnafu},
    message::{
        Commit, CommitVote, ConsensusMessage, Decide, NewView, PreCommit, PreCommitVote, Prepare,
        PrepareVote,
    },
    traits::{
        node_implementation::{NodeImplementation, TypeMap},
        storage::Storage,
    },
};
use snafu::ResultExt;

/// The update context that is used by the different phases.
#[derive(custom_debug::Debug)]
pub(super) struct UpdateCtx<'a, I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize> {
    /// A reference to the [`ConsensusApi`] for e.g. loading information, getting config or sending messages.
    #[debug(skip)]
    pub(super) api: &'a mut A,
    /// The current view number
    pub(super) view_number: ViewNumber,
    /// All transactions that have been received. These will also include the transactions that have been proposed.
    pub(super) transactions: &'a [TransactionState<I, N>],
    /// All messages that have been received this round
    pub(super) messages: &'a mut Vec<<I as TypeMap<N>>::ConsensusMessage>,
    /// `true` if this phase is leader in this round.
    pub(super) is_leader: bool,
    /// The current stage of this phase
    pub(super) stage: Stage,
}

impl<'a, I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize> UpdateCtx<'a, I, A, N> {
    /// Get a leaf by its own [`LeafHash`]
    ///
    /// # Errors
    ///
    /// Will return an error when the underlying storage returns an error
    pub(super) async fn get_leaf(&self, leaf_hash: &LeafHash<N>) -> Result<Leaf<I::Block, N>> {
        self.api
            .storage()
            .get_leaf(leaf_hash)
            .await
            .context(StorageSnafu)?
            .or_not_found(leaf_hash)
    }

    /// Send a broadcast message to the [`ConsensusApi`] and update the internal message list.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying [`ConsensusApi::send_broadcast_message`] returns an error.
    pub(super) async fn send_broadcast_message(
        &mut self,
        message: <I as TypeMap<N>>::ConsensusMessage,
    ) -> Result {
        self.api
            .send_broadcast_message(message.clone())
            .await
            .context(FailedToBroadcastSnafu { stage: self.stage })?;
        // If the networking layer sends this message to ourselves, that means this message will be inserted twice
        // TODO: Make sure this doesn't happen
        self.messages.push(message);
        Ok(())
    }

    /// Get a leaf by the given [`BlockHash`]
    ///
    /// # Errors
    ///
    /// Will return an error when the underlying storage returns an error
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

    /// Get a state by the given [`LeafHash`]
    ///
    /// # Errors
    ///
    /// Will return an error when the underlying storage returns an error
    pub(super) async fn get_state_by_leaf(&self, leaf: &LeafHash<N>) -> Result<I::State> {
        self.api
            .storage()
            .get_state(leaf)
            .await
            .context(StorageSnafu)?
            .or_not_found(leaf)
    }

    /// Get the newest QC from the storage.
    ///
    /// # Errors
    ///
    /// Will return an error when the underlying storage returns an error
    pub(super) async fn get_newest_qc(&self) -> Result<Option<QuorumCertificate<N>>> {
        self.api
            .storage()
            .get_newest_qc()
            .await
            .context(StorageSnafu)
    }

    /// Get all messages that match filter `filter`
    fn messages<'this, FN, RET>(&'this self, filter: FN) -> impl Iterator<Item = RET> + 'this
    where
        FN: FnMut(&'this <I as TypeMap<N>>::ConsensusMessage) -> Option<RET> + 'this,
    {
        self.messages.iter().filter_map(filter)
    }

    /// Get all [`NewView`] messages that were received
    pub(super) fn new_view_messages(&self) -> impl Iterator<Item = &NewView<N>> + '_ {
        self.messages(|m| {
            if let ConsensusMessage::NewView(nv) = m {
                Some(nv)
            } else {
                None
            }
        })
    }

    /// Get the first [`Prepare`] message that was received.
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

    /// Get all [`PrepareVote`] messages that were received
    pub(super) fn prepare_vote_messages(&self) -> impl Iterator<Item = &PrepareVote<N>> + '_ {
        self.messages(|m| {
            if let ConsensusMessage::PrepareVote(prepare) = m {
                Some(prepare)
            } else {
                None
            }
        })
    }

    /// Get the first [`PreCommit`] message that was received.
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

    /// Get all [`PreCommitVote`] messages that were received
    pub(super) fn pre_commit_vote_messages(&self) -> impl Iterator<Item = &PreCommitVote<N>> + '_ {
        self.messages(|m| {
            if let ConsensusMessage::PreCommitVote(prepare) = m {
                Some(prepare)
            } else {
                None
            }
        })
    }

    /// Get the first [`Commit`] message that was received.
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

    /// Get all [`CommitVote`] messages that were received
    pub(super) fn commit_vote_messages(&self) -> impl Iterator<Item = &CommitVote<N>> + '_ {
        self.messages(|m| {
            if let ConsensusMessage::CommitVote(prepare) = m {
                Some(prepare)
            } else {
                None
            }
        })
    }

    /// Get the first [`Decide`] message that was received.
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
}
