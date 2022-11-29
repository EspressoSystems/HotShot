//! Contains the [`NextLeader`] struct used for the next leader step in the hotstuff consensus algorithm.

use crate::ConsensusApi;
use async_lock::Mutex;
use hotshot_types::data::{ValidatingLeaf, ValidatingProposal};
use hotshot_types::traits::election::Checked::Unchecked;
use hotshot_types::traits::election::VoteToken;
use hotshot_types::traits::node_implementation::NodeTypes;
use hotshot_types::{data::QuorumCertificate, message::ConsensusMessage};
use hotshot_utils::channel::UnboundedReceiver;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};
use tracing::{error, instrument, warn};

/// The next view's leader
#[derive(Debug, Clone)]
pub struct NextLeader<A: ConsensusApi<TYPES, ValidatingLeaf<TYPES>, ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>>, TYPES: NodeTypes> {
    /// id of node
    pub id: u64,
    /// generic_qc before starting this
    pub generic_qc: QuorumCertificate<TYPES>,
    /// channel through which the leader collects votes
    pub vote_collection_chan: Arc<Mutex<UnboundedReceiver<ConsensusMessage<TYPES>>>>,
    /// The view number we're running on
    pub cur_view: TYPES::Time,
    /// Limited access to the consensus protocol
    pub api: A,
}

impl<A: ConsensusApi<TYPES>, TYPES: NodeTypes> NextLeader<A, TYPES> {
    /// Run one view of the next leader task
    /// # Panics
    /// While we are unwrapping, this function can logically never panic
    /// unless there is a bug in std
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Next Leader Task", level = "error")]
    pub async fn run_view(self) -> QuorumCertificate<TYPES> {
        error!("Next Leader task started!");
        let mut qcs = HashSet::<QuorumCertificate<TYPES>>::new();
        qcs.insert(self.generic_qc.clone());

        let mut vote_outcomes = HashMap::new();

        let threshold = self.api.threshold();
        let mut stake_casted = 0;

        let lock = self.vote_collection_chan.lock().await;
        while let Ok(msg) = lock.recv().await {
            if msg.view_number() != self.cur_view {
                continue;
            }
            match msg {
                ConsensusMessage::TimedOut(t) => {
                    qcs.insert(t.justify_qc);
                }
                ConsensusMessage::Vote(vote) => {
                    // if the signature on the vote is invalid,
                    // assume it's sent by byzantine node
                    // and ignore

                    if !self.api.is_valid_signature(
                        &vote.signature.0,
                        &vote.signature.1,
                        vote.leaf_commitment,
                        vote.current_view,
                        // Ignoring deserialization errors below since we are getting rid of it soon
                        Unchecked(vote.vote_token.clone()),
                    ) {
                        continue;
                    }

                    // TODO ed ensure we have the QC that the QC commitment references

                    let (_bh, map) = vote_outcomes
                        .entry(vote.leaf_commitment)
                        .or_insert_with(|| (vote.block_commitment, BTreeMap::new()));
                    map.insert(
                        vote.signature.0.clone(),
                        (vote.signature.1.clone(), vote.vote_token.clone()),
                    );

                    stake_casted += u64::from(vote.vote_token.vote_count());

                    if stake_casted >= u64::from(threshold) {
                        let (block_commitment, valid_signatures) =
                            vote_outcomes.remove(&vote.leaf_commitment).unwrap();

                        // construct QC
                        let qc = QuorumCertificate {
                            block_commitment,
                            leaf_commitment: vote.leaf_commitment,
                            view_number: self.cur_view,
                            signatures: valid_signatures,
                            genesis: false,
                        };
                        return qc;
                    }
                }
                ConsensusMessage::NextViewInterrupt(_view_number) => {
                    self.api.send_next_leader_timeout(self.cur_view).await;
                    break;
                }
                ConsensusMessage::Proposal(_p) => {
                    warn!("The next leader has received an unexpected proposal!");
                }
            }
        }

        qcs.into_iter().max_by_key(|qc| qc.view_number).unwrap()
    }
}
