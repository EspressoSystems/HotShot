//! Contains the [`NextLeader`] struct used for the next leader step in the hotstuff consensus algorithm.

use crate::{utils::Signatures, ConsensusApi};
use async_lock::Mutex;
use commit::Commitment;
use hotshot_types::{
    data::{Leaf, QuorumCertificate, ViewNumber},
    message::ConsensusMessage,
    traits::{node_implementation::NodeImplementation, StateContents},
};
use hotshot_utils::channel::UnboundedReceiver;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};
use tracing::{info, instrument, warn};

/// The next view's leader
#[derive(Debug, Clone)]
pub struct NextLeader<A: ConsensusApi<I>, I: NodeImplementation> {
    /// id of node
    pub id: u64,
    /// generic_qc before starting this
    pub generic_qc: QuorumCertificate<I::State>,
    /// channel through which the leader collects votes
    pub vote_collection_chan: Arc<Mutex<UnboundedReceiver<ConsensusMessage<I::State>>>>,
    /// The view number we're running on
    pub cur_view: ViewNumber,
    /// Limited access to the consensus protocol
    pub api: A,
}

impl<A: ConsensusApi<I>, I: NodeImplementation> NextLeader<A, I> {
    /// Run one view of the next leader task
    /// # Panics
    /// While we are unwrapping, this function can logically never panic
    /// unless there is a bug in std
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Next Leader Task", level = "error")]
    pub async fn run_view(self) -> QuorumCertificate<I::State> {
        info!("Next Leader task started!");
        let mut qcs = HashSet::<QuorumCertificate<I::State>>::new();
        qcs.insert(self.generic_qc.clone());

        #[allow(clippy::type_complexity)]
        let mut vote_outcomes: HashMap<
            Commitment<Leaf<I::State>>,
            (Commitment<<I::State as StateContents>::Block>, Signatures),
        > = HashMap::new();
        // NOTE will need to refactor this during VRF integration
        let threshold = self.api.threshold();

        let lock = self.vote_collection_chan.lock().await;
        while let Ok(msg) = lock.recv().await {
            info!("recv-ed message {:?}", msg.clone());
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
                    ) {
                        continue;
                    }

                    qcs.insert(vote.justify_qc);

                    match vote_outcomes.entry(vote.leaf_commitment) {
                        std::collections::hash_map::Entry::Occupied(mut o) => {
                            let (bh, map) = o.get_mut();
                            if *bh != vote.block_commitment {
                                warn!("Mismatch between commitments in received votes. This is probably an error without byzantine nodes.");
                            }
                            map.insert(vote.signature.0.clone(), vote.signature.1.clone());
                        }
                        std::collections::hash_map::Entry::Vacant(location) => {
                            let mut map = BTreeMap::new();
                            map.insert(vote.signature.0, vote.signature.1);
                            location.insert((vote.block_commitment, map));
                        }
                    }

                    // unwraps here are fine since we *just* inserted the key
                    let (_, valid_signatures) = vote_outcomes.get(&vote.leaf_commitment).unwrap();

                    if valid_signatures.len() >= threshold.into() {
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
