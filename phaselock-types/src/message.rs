//! Network message types
//!
//! This module contains types used to represent the various types of messages that
//! `PhaseLock` nodes can send among themselves.

use crate::{
    data::{Leaf, LeafHash, QuorumCertificate, Stage, ViewNumber},
    PubKey,
};
use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use threshold_crypto::{PublicKeySet, SignatureShare};

/// Incoming message
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message<B, T, S, const N: usize> {
    /// The sender of this message
    pub sender: PubKey,

    /// The message kind
    pub kind: MessageKind<B, T, S, N>,
}

/// Enum representation of any message type
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MessageKind<B, T, S, const N: usize> {
    /// Messages related to the consensus protocol
    Consensus(ConsensusMessage<B, S, N>),
    /// Messages relating to sharing data between nodes
    Data(DataMessage<B, T, S, N>),
}

impl<B, T, S, const N: usize> From<ConsensusMessage<B, S, N>> for MessageKind<B, T, S, N> {
    fn from(m: ConsensusMessage<B, S, N>) -> Self {
        Self::Consensus(m)
    }
}

impl<B, T, S, const N: usize> From<DataMessage<B, T, S, N>> for MessageKind<B, T, S, N> {
    fn from(m: DataMessage<B, T, S, N>) -> Self {
        Self::Data(m)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
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
    pub fn view_number(&self) -> ViewNumber {
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
            ConsensusMessage::PreCommit(pre_commit) => {
                // PreCommit QC has the votes of the Prepare phase, therefor we must compare against Prepare and not PreCommit
                (&pre_commit.qc, pre_commit.current_view, Stage::Prepare)
            }
            // Same as PreCommit, we compare with 1 stage earlier
            ConsensusMessage::Commit(commit) => (&commit.qc, commit.current_view, Stage::PreCommit),
            ConsensusMessage::Decide(decide) => (&decide.qc, decide.current_view, Stage::Commit),

            ConsensusMessage::NewView(_)
            | ConsensusMessage::Prepare(_)
            | ConsensusMessage::CommitVote(_)
            | ConsensusMessage::PreCommitVote(_)
            | ConsensusMessage::PrepareVote(_) => return true,
        };

        qc.verify(public_key, view_number, stage)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
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

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
/// Signals the start of a new view
pub struct NewView<const N: usize> {
    /// The current view
    pub current_view: ViewNumber,
    /// The justification qc for this view
    pub justify: QuorumCertificate<N>,
}

#[derive(Serialize, Deserialize, Clone, Debug, std::hash::Hash, PartialEq, Eq)]
/// Prepare qc from the leader
pub struct Prepare<B, S, const N: usize> {
    /// The current view
    pub current_view: ViewNumber,
    /// The item being proposed
    pub leaf: Leaf<B, N>,
    /// The state this proposal results in
    pub state: S,
    /// The current high qc
    pub high_qc: QuorumCertificate<N>,
}

/// A nodes vote on the prepare field.
///
/// This should not be used directly. Consider using [`PrepareVote`], [`PreCommitVote`] or [`CommitVote`] instead.
#[derive(Serialize, Deserialize, Clone, custom_debug::Debug, std::hash::Hash, PartialEq, Eq)]
/// A nodes vote on the prepare field
pub struct Vote<const N: usize> {
    /// The signature share associated with this vote
    pub signature: SignatureShare,
    /// Id of the voting nodes
    pub id: u64,
    /// Hash of the item being voted on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: LeafHash<N>,
    /// The view this vote was cast for
    pub current_view: ViewNumber,
}

/// Generate a wrapper for [`Vote`] for type safety.
macro_rules! vote_wrapper {
    ($name:ident) => {
        /// Wrapper around [`Vote`], used for type safety.
        #[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, std::hash::Hash)]
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

#[derive(Serialize, Deserialize, Clone, custom_debug::Debug, std::hash::Hash, PartialEq, Eq)]
/// Pre-commit qc from the leader
pub struct PreCommit<const N: usize> {
    /// Hash of the item being worked on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: LeafHash<N>,
    /// The pre commit qc
    pub qc: QuorumCertificate<N>,
    /// The current view
    pub current_view: ViewNumber,
}

#[derive(Serialize, Deserialize, Clone, custom_debug::Debug, std::hash::Hash, PartialEq, Eq)]
/// `Commit` qc from the leader
pub struct Commit<const N: usize> {
    /// Hash of the thing being worked on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: LeafHash<N>,
    /// The `Commit` qc
    pub qc: QuorumCertificate<N>,
    /// The current view
    pub current_view: ViewNumber,
}

#[derive(Serialize, Deserialize, Clone, custom_debug::Debug, std::hash::Hash, PartialEq, Eq)]
/// Final decision
pub struct Decide<const N: usize> {
    /// Hash of the thing we just decided on
    #[debug(with = "fmt_leaf_hash")]
    pub leaf_hash: LeafHash<N>,
    /// final qc for the round
    pub qc: QuorumCertificate<N>,
    /// the current view
    pub current_view: ViewNumber,
}

/// Format a [`LeafHash`] with [`HexFmt`]
fn fmt_leaf_hash<const N: usize>(
    n: &LeafHash<N>,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    write!(f, "{:12}", HexFmt(n))
}
