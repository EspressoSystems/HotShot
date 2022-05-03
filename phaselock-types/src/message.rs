//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `PhaseLock` nodes can send among themselves.

use crate::data::{Leaf, LeafHash, QuorumCertificate, Stage};
use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use threshold_crypto::{PublicKeySet, SignatureShare};

#[derive(Serialize, Deserialize, Clone, Debug)]
/// Enum representation of any message type
pub enum Message<B, T, S, const N: usize> {
    /// Messages related to the consensus protocol
    Consensus(ConsensusMessage<B, S, N>),
    /// Messages relating to sharing data between nodes
    Data(DataMessage<B, T, S, N>),
}

impl<B, T, S, const N: usize> From<ConsensusMessage<B, S, N>> for Message<B, T, S, N> {
    fn from(m: ConsensusMessage<B, S, N>) -> Self {
        Self::Consensus(m)
    }
}

impl<B, T, S, const N: usize> From<DataMessage<B, T, S, N>> for Message<B, T, S, N> {
    fn from(m: DataMessage<B, T, S, N>) -> Self {
        Self::Data(m)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
/// Messages related to the consensus protocol
pub enum ConsensusMessage<B, S, const N: usize> {
    /// Signals start of a new view
    NewView(NewView<N>),
    /// Contains the prepare qc from the leader
    Prepare(Prepare<B, S, N>),
    /// A nodes vote on the prepare stage
    PrepareVote(PrepareVote<N>),
    /// Contains the precommit qc from the leader
    PreCommit(PreCommit<N>),
    /// A node's vote on the precommit stage
    PreCommitVote(PreCommitVote<N>),
    /// Contains the commit qc from the leader
    Commit(Commit<N>),
    /// A node's vote on the commit stage
    CommitVote(CommitVote<N>),
    /// Contains the decide qc from the leader
    Decide(Decide<N>),
}

impl<B, S, const N: usize> ConsensusMessage<B, S, N> {
    /// Get the current view number from this message.
    /// If this message is `SubmitTransaction` the returned value will be `None`.
    /// Otherwise the return value will be the `current_view` of the inner struct.
    pub fn view_number(&self) -> u64 {
        match self {
            Self::NewView(view) => view.current_view,
            Self::Prepare(prepare) => prepare.current_view,
            Self::PrepareVote(vote) => vote.current_view,
            Self::PreCommit(precommit) => precommit.current_view,
            Self::PreCommitVote(vote) => vote.current_view,
            Self::Commit(commit) => commit.current_view,
            Self::CommitVote(vote) => vote.current_view,
            Self::Decide(decide) => decide.current_view,
        }
    }

    /// Validate this message on if the QC is correct, if it has one
    ///
    /// If this message has no QC then this will return `true`
    pub fn validate_qc(&self, public_key: &PublicKeySet) -> bool {
        let (qc, view_number, stage) = match self {
            ConsensusMessage::NewView(view) => (&view.justify, view.current_view, Stage::Prepare),
            ConsensusMessage::Prepare(prepare) => {
                (&prepare.high_qc, prepare.current_view, Stage::Prepare)
            }
            ConsensusMessage::PreCommit(_pre_commit) => {
                // TODO(vko): This seems to always fail
                // (&pre_commit.qc, pre_commit.current_view, Stage::PreCommit)
                return true;
            }
            ConsensusMessage::Commit(_commit) => {
                // TODO(vko): This seems to always fail
                // (&commit.qc, commit.current_view, Stage::Commit)
                return true;
            }
            ConsensusMessage::Decide(_decide) => {
                // TODO(vko): This seems to always fail
                // (&decide.qc, decide.current_view, Stage::Decide)
                return true;
            }

            ConsensusMessage::CommitVote(_)
            | ConsensusMessage::PreCommitVote(_)
            | ConsensusMessage::PrepareVote(_) => return true,
        };

        qc.verify(public_key, view_number, stage)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
/// Messages related to sending data between nodes
pub enum DataMessage<B, T, S, const N: usize> {
    /// The newest entry that a node knows. This is send from existing nodes to a new node when the new node joins the network
    NewestQuorumCertificate {
        /// The newest [`QuorumCertificate`]
        quorum_certificate: QuorumCertificate<N>,
        /// The relevant [`BlockContents`]
        ///
        /// [`BlockContents`]: ../traits/block_contents/trait.BlockContents.html
        block: B,
        /// The relevant [`State`]
        ///
        /// [`State`]: ../traits/state/trait.State.html
        state: S,
    },
    /// Contains a transaction to be submitted
    SubmitTransaction(T),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// Signals the start of a new view
pub struct NewView<const N: usize> {
    /// The current view
    pub current_view: u64,
    /// The justification qc for this view
    pub justify: QuorumCertificate<N>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// Prepare qc from the leader
pub struct Prepare<B, S, const N: usize> {
    /// The current view
    pub current_view: u64,
    /// The item being proposed
    pub leaf: Leaf<B, N>,
    /// The state this proposal results in
    pub state: S,
    /// The current high qc
    pub high_qc: QuorumCertificate<N>,
}

#[derive(Serialize, Deserialize, Clone, custom_debug::Debug)]
/// A nodes vote on the prepare field.
///
/// This should not be used directly. Consider using [`PrepareVote`], [`PreCommitVote`] or [`CommitVote`] instead.
pub struct Vote<const N: usize> {
    /// The signature share associated with this vote
    pub signature: SignatureShare,
    /// Id of the voting nodes
    pub id: u64,
    /// Hash of the item being voted on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: LeafHash<N>,
    /// The view this vote was cast for
    pub current_view: u64,
}

/// Generate a wrapper for [`Vote`] for type safety.
macro_rules! vote_wrapper {
    ($name:ident) => {
        /// Wrapper around [`Vote`], used for type safety.
        #[derive(Serialize, Deserialize, Clone, Debug)]
        pub struct $name<const N: usize>(pub Vote<N>);
        impl<const N: usize> std::ops::Deref for $name<N> {
            type Target = Vote<N>;
            fn deref(&self) -> &Vote<N> {
                &self.0
            }
        }

        impl<const N: usize> From<Vote<N>> for $name<N> {
            fn from(v: Vote<N>) -> Self {
                Self(v)
            }
        }

        impl<const N: usize> From<$name<N>> for Vote<N> {
            fn from(wrapper: $name<N>) -> Self {
                wrapper.0
            }
        }
    };
}

vote_wrapper!(PrepareVote);
vote_wrapper!(PreCommitVote);
vote_wrapper!(CommitVote);

#[derive(Serialize, Deserialize, Clone, custom_debug::Debug)]
/// Pre-commit qc from the leader
pub struct PreCommit<const N: usize> {
    /// Hash of the item being worked on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: LeafHash<N>,
    /// The pre commit qc
    pub qc: QuorumCertificate<N>,
    /// The current view
    pub current_view: u64,
}

#[derive(Serialize, Deserialize, Clone, custom_debug::Debug)]
/// `Commit` qc from the leader
pub struct Commit<const N: usize> {
    /// Hash of the thing being worked on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: LeafHash<N>,
    /// The `Commit` qc
    pub qc: QuorumCertificate<N>,
    /// The current view
    pub current_view: u64,
}

#[derive(Serialize, Deserialize, Clone, custom_debug::Debug)]
/// Final decision
pub struct Decide<const N: usize> {
    /// Hash of the thing we just decided on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: LeafHash<N>,
    /// final qc for the round
    pub qc: QuorumCertificate<N>,
    /// the current view
    pub current_view: u64,
}

/// Format a [`LeafHash`] with [`HexFmt`]
fn fmt_leaf_hash<const N: usize>(
    n: &LeafHash<N>,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    write!(f, "{:12}", HexFmt(n))
}
