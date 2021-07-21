use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};
use threshold_crypto::SignatureShare;

use std::fmt::Debug;

use crate::{data::Leaf, BlockHash, PubKey, QuorumCertificate};

#[derive(Serialize, Deserialize, Clone, Debug)]
/// Represents the messages `PhaseLock` nodes send to each other
pub enum Message<B, T, S, const N: usize> {
    /// Signals start of a new view
    NewView(NewView<N>),
    /// Contains the prepare qc from the leader
    Prepare(Prepare<B, N>),
    /// A nodes vote on the prepare stage
    PrepareVote(Vote<N>),
    /// Contains the precommit qc from the leader
    PreCommit(PreCommit<N>),
    /// A node's vote on the precommit stage
    PreCommitVote(Vote<N>),
    /// Contains the commit qc from the leader
    Commit(Commit<N>),
    /// A node's vote on the commit stage
    CommitVote(Vote<N>),
    /// Contains the decide qc from the leader
    Decide(Decide<N>),
    /// Contains a transaction to be submitted
    SubmitTransaction(T),
    /// Contains a [`Query`]
    Query(Query<N>),
    /// Contains a [`Response`]
    Response(Response<B, S, N>),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// Signals the start of a new view
pub struct NewView<const N: usize> {
    /// The current view
    pub current_view: u64,
    /// The justification qc for this view
    pub justify: super::QuorumCertificate<N>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
/// Prepare qc from the leader
pub struct Prepare<T, const N: usize> {
    /// The current view
    pub current_view: u64,
    /// The item being proposed
    pub leaf: Leaf<T, N>,
    /// The current high qc
    pub high_qc: QuorumCertificate<N>,
}

#[derive(Serialize, Deserialize, Clone)]
/// A nodes vote on the prepare field
pub struct Vote<const N: usize> {
    /// The signature share associated with this vote
    pub signature: SignatureShare,
    /// Id of the voting nodes
    pub id: u64,
    /// Hash of the item being voted on
    pub leaf_hash: BlockHash<N>,
    /// The view this vote was cast for
    pub current_view: u64,
}

impl<const N: usize> Debug for Vote<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrepareVote")
            .field("current_view", &self.current_view)
            .field("signature", &self.signature)
            .field("id", &self.id)
            .field("leaf_hash", &format!("{:12}", HexFmt(&self.leaf_hash)))
            .finish()
    }
}

#[derive(Serialize, Deserialize, Clone)]
/// Pre-commit qc from the leader
pub struct PreCommit<const N: usize> {
    /// Hash of the item being worked on
    pub leaf_hash: BlockHash<N>,
    /// The pre commit qc
    pub qc: QuorumCertificate<N>,
    /// The current view
    pub current_view: u64,
}

impl<const N: usize> Debug for PreCommit<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreCommit")
            .field("current_view", &self.current_view)
            .field("qc", &self.qc)
            .field("leaf_hash", &format!("{:12}", HexFmt(&self.leaf_hash)))
            .finish()
    }
}

#[derive(Serialize, Deserialize, Clone)]
/// `Commit` qc from the leader
pub struct Commit<const N: usize> {
    /// Hash of the thing being worked on
    pub leaf_hash: BlockHash<N>,
    /// The `Commit` qc
    pub qc: QuorumCertificate<N>,
    /// The current view
    pub current_view: u64,
}

impl<const N: usize> Debug for Commit<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Commit")
            .field("current_view", &self.current_view)
            .field("qc", &self.qc)
            .field("leaf_hash", &format!("{:12}", HexFmt(&self.leaf_hash)))
            .finish()
    }
}

#[derive(Serialize, Deserialize, Clone)]
/// Final decision
pub struct Decide<const N: usize> {
    /// Hash of the thing we just decided on
    pub leaf_hash: BlockHash<N>,
    /// final qc for the round
    pub qc: QuorumCertificate<N>,
    /// the current view
    pub current_view: u64,
}

impl<const N: usize> Debug for Decide<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decide")
            .field("current_view", &self.current_view)
            .field("qc", &self.qc)
            .field("leaf_hash", &format!("{:12}", HexFmt(&self.leaf_hash)))
            .finish()
    }
}

/// Describes the type of queries that can be made of a [`PhaseLock`](crate::PhaseLock)
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum QueryType<const N: usize> {
    /// Request a particular block from a [`PhaseLock`](crate::PhaseLock)
    Block {
        /// The hash of the block
        hash: BlockHash<N>,
    },
    /// Request a particular historical state from a [`PhaseLock`](crate::PhaseLock)
    State {
        /// The hash of the state
        hash: BlockHash<N>,
    },
    /// Request the node's current view number
    ViewNumber,
    /// Request a particular [`Leaf`] from a [`PhaseLock`](crate::PhaseLock) node
    Leaf {
        /// The hash of the [`Leaf`]
        hash: BlockHash<N>,
    },
    /// Request the [`QuorumCertificate`](crate::data::QuorumCertificate) for a particular leaf
    QuorumCertificate {
        /// The hash of the [`Leaf`] we want the QC for
        hash: BlockHash<N>,
    },
}

/// Describes a singular query being sent to a [`PhaseLock`](crate::PhaseLock)
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Query<const N: usize> {
    /// The type and other data for the query
    pub query: QueryType<N>,
    /// The public key of the sender
    pub sender: PubKey,
    /// A discriminator value used by the sender to tell messages apart
    pub nonce: u64,
}

/// Describes the type of response to a query
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ResponseType<B, S, const N: usize> {
    /// The requested block
    Block {
        /// The block itself
        block: B,
    },
    /// The requested state
    State {
        /// The state itself
        state: S,
    },
    /// The current view number
    ViewNumber {
        /// Our current view number
        view: u64,
    },
    /// The requested leaf
    Leaf {
        /// The leaf itself
        leaf: Leaf<B, N>,
    },
    /// The requested quorum certificate
    QuorumCertificate {
        /// the [`QuorumCertificate`] itself
        qc: QuorumCertificate<N>,
    },
}

/// Describes a response to a given query
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Response<B, S, const N: usize> {
    /// The response proper
    ///
    /// Will be `None` if the query sender asked for data we do not have
    pub response: Option<ResponseType<B, S, N>>,
    /// The public key of the node sending this response
    pub sender: PubKey,
    /// The discriminator
    ///
    /// Needs to be the same as the discriminator sent in the initial [`Query`]
    pub nonce: u64,
}
