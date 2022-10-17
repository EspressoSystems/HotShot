//! Contains the [`NextLeader`] struct used for the next leader step in the hotstuff consensus algorithm.

use crate::ConsensusApi;
use async_lock::Mutex;
use hotshot_types::traits::election::Checked::Unchecked;
use hotshot_types::traits::node_implementation::NodeTypes;
use hotshot_types::{data::QuorumCertificate, message::ConsensusMessage};
use hotshot_utils::channel::UnboundedReceiver;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};
use tracing::{info, instrument, warn};

/// The next view's leader
#[derive(Debug, Clone)]
pub struct NextLeader<A: ConsensusApi<TYPES>, TYPES: NodeTypes> {
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
        info!("Next Leader task started!");
        let mut qcs = HashSet::<QuorumCertificate<TYPES>>::new();
        qcs.insert(self.generic_qc.clone());

        let mut vote_outcomes = HashMap::new();
        // TODO will need to refactor this during VRF integration
        let threshold = self.api.threshold();

        let lock = self.vote_collection_chan.lock().await;
        while let Ok(msg) = lock.recv().await {
            if msg.time() != &self.cur_view {
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
                        vote.time,
                        // Ignoring deserialization errors below since we are getting rid of it soon
                        Unchecked(vote.vote_token.clone()),
                    ) {
                        continue;
                    }

                    qcs.insert(vote.justify_qc);

                    let (_bh, map) = vote_outcomes
                        .entry(vote.leaf_commitment)
                        .or_insert_with(|| (vote.block_commitment, BTreeMap::new()));
                    map.insert(
                        vote.signature.0.clone(),
                        (vote.signature.1.clone(), vote.vote_token),
                    );
                    let valid_signatures = map.clone();

                    // TODO ed - current validated_stake rechecks that all votes are valid, which isn't necessary here
                    let stake = self.api.validated_stake(
                        vote.leaf_commitment,
                        self.cur_view.clone(),
                        valid_signatures,
                    );
                    let stake_casted: usize = stake.try_into().unwrap();
                    if stake_casted >= threshold.into() {
                        let (block_commitment, valid_signatures) =
                            vote_outcomes.remove(&vote.leaf_commitment).unwrap();
                        // construct QC
                        let qc = QuorumCertificate {
                            block_commitment,
                            leaf_commitment: vote.leaf_commitment,
                            time: self.cur_view.clone(),
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

        qcs.into_iter().max_by_key(|qc| qc.time.clone()).unwrap()
    }
}
