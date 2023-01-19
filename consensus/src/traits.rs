//! Contains the [`ConsensusApi`] trait.

use crate::Consensus;
use async_trait::async_trait;
use commit::Commitment;
use hotshot_types::data::DAProposal;
use hotshot_types::message::ConsensusMessage;
use hotshot_types::traits::election::Election;
use hotshot_types::traits::node_implementation::NodeType;
use hotshot_types::traits::storage::StorageError;
use hotshot_types::{
    data::{LeafType, ProposalType},
    error::HotShotError,
    event::{Event, EventType},
    traits::{
        election::{Checked, ElectionError, VoteData},
        network::NetworkError,
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
    },
};
use std::num::NonZeroU64;
use std::{num::NonZeroUsize, sync::Arc, time::Duration};

// FIXME these should be nonzero u64s
/// The API that [`HotStuff`] needs to talk to the system. This should be implemented in the `hotshot` crate and passed to all functions on `HotStuff`.
///
/// [`HotStuff`]: struct.HotStuff.html
#[async_trait]
pub trait ConsensusApi<
    TYPES: NodeType,
    LEAF: LeafType<NodeType = TYPES>,
    PROPOSAL: ProposalType<NodeType = TYPES>,
>: Send + Sync
{
    /// Total number of nodes in the network. Also known as `n`.
    fn total_nodes(&self) -> NonZeroUsize;

    /// The amount of stake required to reach a decision. See implementation of `Election` for more details.
    fn threshold(&self) -> NonZeroU64;

    /// The minimum amount of time a leader has to wait before sending a propose
    fn propose_min_round_time(&self) -> Duration;

    /// The maximum amount of time a leader can wait before sending a propose.ConsensusApi
    /// If this time is reached, the leader has to send a propose without transactions.
    fn propose_max_round_time(&self) -> Duration;

    /// Store a leaf in the storage
    async fn store_leaf(
        &self,
        old_anchor_view: TYPES::Time,
        leaf: LEAF,
    ) -> Result<(), StorageError>;

    /// Retuns the maximum transactions allowed in a block
    fn max_transactions(&self) -> NonZeroUsize;

    /// Returns the minimum transactions that must be in a block
    fn min_transactions(&self) -> usize;

    /// Generates and encodes a vote token
    #[allow(clippy::type_complexity)]
    fn make_vote_token(
        &self,
        view_number: TYPES::Time,
    ) -> Result<Option<TYPES::VoteTokenType>, ElectionError>;

    /// Returns the `I::SignatureKey` of the leader for the given round and stage
    async fn get_leader(&self, view_number: TYPES::Time) -> TYPES::SignatureKey;

    /// Returns `true` if hotstuff should start the given round. A round can also be started manually by sending `NewView` to the leader.
    ///
    /// In production code this should probably always return `true`.
    async fn should_start_round(&self, view_number: TYPES::Time) -> bool;

    /// Send a direct message to the given recipient
    async fn send_direct_message(
        &self,
        recipient: TYPES::SignatureKey,
        message: ConsensusMessage<TYPES, LEAF, PROPOSAL>,
    ) -> std::result::Result<(), NetworkError>;

    /// Send a broadcast message to the entire network.
    async fn send_broadcast_message(
        &self,
        message: ConsensusMessage<TYPES, LEAF, PROPOSAL>,
    ) -> std::result::Result<(), NetworkError>;

    /// Notify the system of an event within `hotshot-consensus`.
    async fn send_event(&self, event: Event<TYPES, LEAF>);

    /// Get a reference to the public key.
    fn public_key(&self) -> &TYPES::SignatureKey;

    /// Get a reference to the private key.
    fn private_key(&self) -> &<TYPES::SignatureKey as SignatureKey>::PrivateKey;

    // Utility functions

    /// returns `true` if the current node is a leader for the given `view_number`
    async fn is_leader(&self, view_number: TYPES::Time) -> bool {
        &self.get_leader(view_number).await == self.public_key()
    }

    /// notifies client of an error
    async fn send_view_error(&self, view_number: TYPES::Time, error: Arc<HotShotError<TYPES>>) {
        self.send_event(Event {
            view_number,
            event: EventType::Error { error },
        })
        .await;
    }

    /// notifies client of a replica timeout
    async fn send_replica_timeout(&self, view_number: TYPES::Time) {
        self.send_event(Event {
            view_number,
            event: EventType::ReplicaViewTimeout { view_number },
        })
        .await;
    }

    /// notifies client of a next leader timeout
    async fn send_next_leader_timeout(&self, view_number: TYPES::Time) {
        self.send_event(Event {
            view_number,
            event: EventType::NextLeaderViewTimeout { view_number },
        })
        .await;
    }

    /// sends a decide event down the channel
    async fn send_decide(
        &self,
        view_number: TYPES::Time,
        leaf_views: Vec<LEAF>,
        decide_qc: LEAF::QuorumCertificate,
    ) {
        self.send_event(Event {
            view_number,
            event: EventType::Decide {
                leaf_chain: Arc::new(leaf_views),
                qc: Arc::new(decide_qc),
            },
        })
        .await;
    }

    /// Sends a `ViewFinished` event
    async fn send_view_finished(&self, view_number: TYPES::Time) {
        self.send_event(Event {
            view_number,
            event: EventType::ViewFinished { view_number },
        })
        .await;
    }

    async fn send_da_broadcast<DAPROPOSAL: ProposalType<NodeType = TYPES>>(
        &self,
        message: ConsensusMessage<TYPES, LEAF, DAPROPOSAL>,
    ) -> std::result::Result<(), NetworkError>;

    /// Sign a DA proposal.
    fn sign_da_proposal(
        &self,
        block_commitment: &Commitment<TYPES::BlockType>,
    ) -> EncodedSignature {
        let signature = TYPES::SignatureKey::sign(self.private_key(), block_commitment.as_ref());
        signature
    }

    /// Sign a validating or commitment proposal.
    fn sign_validating_or_commitment_proposal(
        &self,
        leaf_commitment: &Commitment<LEAF>,
    ) -> EncodedSignature {
        let signature = TYPES::SignatureKey::sign(self.private_key(), leaf_commitment.as_ref());
        signature
    }

    /// Sign a vote on DA proposal.
    ///
    /// The block commitment and the type of the vote (DA) are signed, which is the minimum amount
    /// of information necessary for checking that this node voted on that block.
    fn sign_da_vote(
        &self,
        block_commitment: Commitment<TYPES::BlockType>,
    ) -> (EncodedPublicKey, EncodedSignature) {
        let signature = TYPES::SignatureKey::sign(
            self.private_key(),
            &VoteData::<TYPES, LEAF>::DA(block_commitment).as_bytes(),
        );
        (self.public_key().to_bytes(), signature)
    }

    /// Sign a positive vote on validating or commitment proposal.
    ///
    /// The leaf commitment and the type of the vote (yes) are signed, which is the minimum amount
    /// of information necessary for any user of the subsequently constructed QC to check that this
    /// node voted `Yes` on that leaf. The leaf is expected to be reconstructed based on other
    /// information in the yes vote.
    fn sign_yes_vote(
        &self,
        leaf_commitment: Commitment<LEAF>,
    ) -> (EncodedPublicKey, EncodedSignature) {
        let signature = TYPES::SignatureKey::sign(
            self.private_key(),
            &VoteData::<TYPES, LEAF>::Yes(leaf_commitment).as_bytes(),
        );
        (self.public_key().to_bytes(), signature)
    }

    /// Sign a neagtive vote on validating or commitment proposal.
    ///
    /// The leaf commitment and the type of the vote (no) are signed, which is the minimum amount
    /// of information necessary for any user of the subsequently constructed QC to check that this
    /// node voted `No` on that leaf.
    fn sign_no_vote(
        &self,
        leaf_commitment: Commitment<LEAF>,
    ) -> (EncodedPublicKey, EncodedSignature) {
        let signature = TYPES::SignatureKey::sign(
            self.private_key(),
            &VoteData::<TYPES, LEAF>::No(leaf_commitment).as_bytes(),
        );
        (self.public_key().to_bytes(), signature)
    }

    /// Sign a timeout vote.
    ///
    /// We only sign the view number, which is the minimum amount of information necessary for
    /// checking that this node timed out on that view.
    ///
    /// This also allows for the high QC included with the vote to be spoofed in a MITM scenario,
    /// but it is outside our threat model.
    fn sign_timeout_vote(&self, view_number: TYPES::Time) -> (EncodedPublicKey, EncodedSignature) {
        let signature = TYPES::SignatureKey::sign(
            self.private_key(),
            &VoteData::<TYPES, LEAF>::Timeout(view_number).as_bytes(),
        );
        (self.public_key().to_bytes(), signature)
    }

    /// Validate a DAC.
    fn is_valid_dac(
        &self,
        dac: &LEAF::DACertificate,
        block_commitment: Commitment<TYPES::BlockType>,
    ) -> bool;

    /// Validate a QC.
    fn is_valid_qc(&self, qc: &LEAF::QuorumCertificate) -> bool;

    /// Validate a vote.
    fn is_valid_vote(
        &self,
        encoded_key: &EncodedPublicKey,
        encoded_signature: &EncodedSignature,
        data: VoteData<TYPES, LEAF>,
        view_number: TYPES::Time,
        vote_token: Checked<TYPES::VoteTokenType>,
    ) -> bool;
}
