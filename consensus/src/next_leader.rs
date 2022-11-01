//! Contains the [`NextLeader`] struct used for the next leader step in the hotstuff consensus algorithm.

use crate::traits::Signatures;
use crate::ConsensusApi;
use async_lock::Mutex;
use bincode::Options;
use commit::Commitment;
use hotshot_types::traits::election::{Election, VoteToken};
use hotshot_types::traits::election::Checked::Unchecked;
use std::time::Instant;
use hotshot_types::{
    data::{Leaf, QuorumCertificate, ViewNumber},
    message::ConsensusMessage,
    traits::{
        node_implementation::NodeImplementation,
        signature_key::{EncodedPublicKey, EncodedSignature},
        State,
    },
};
use hotshot_utils::{bincode::bincode_opts, channel::UnboundedReceiver};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};
use tracing::{error, info, instrument, warn};

/// The next view's leader
#[derive(Debug, Clone)]
pub struct NextLeader<A: ConsensusApi<I>, I: NodeImplementation> {
    /// id of node
    pub id: u64,
    /// generic_qc before starting this
    pub generic_qc: QuorumCertificate<I::StateType>,
    /// channel through which the leader collects votes
    pub vote_collection_chan: Arc<Mutex<UnboundedReceiver<ConsensusMessage<I::StateType>>>>,
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
    pub async fn run_view(self) -> QuorumCertificate<I::StateType> {
        error!("Next Leader task started!");
        let mut qcs = HashSet::<QuorumCertificate<I::StateType>>::new();
        qcs.insert(self.generic_qc.clone());

        #[allow(clippy::type_complexity)]
        let mut vote_outcomes: HashMap<
            Commitment<Leaf<I::StateType>>,
            (
                Commitment<<I::StateType as State>::BlockType>,
                BTreeMap<EncodedPublicKey, (EncodedSignature, Vec<u8>)>,
            ),
        > = HashMap::new();
        let threshold = self.api.threshold();
        let mut stake_casted = 0;
        let mut signature_map: Signatures<I> = BTreeMap::new();
        let mut num_votes = 0; 

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
                    error!("Matched vote token!");
                    let start = Instant::now(); 
                    num_votes += 1; 
                    // if the signature on the vote is invalid,
                    // assume it's sent by byzantine node
                    // and ignore
                    // TODO ed - ignoring serialization errors since we are changing this type in the future
                    let vote_token: <<I as NodeImplementation>::Election as Election<<I as NodeImplementation>::SignatureKey, ViewNumber>>::VoteTokenType = bincode_opts().deserialize(&vote.vote_token).unwrap();

                    if !self.api.is_valid_signature(
                        &vote.signature.0,
                        &vote.signature.1,
                        vote.leaf_commitment,
                        vote.current_view,
                        // Ignoring deserialization errors below since we are getting rid of it soon
                        Unchecked(vote_token.clone()),
                    ) {
                        info!("Invalid vote token signature");
                        continue;
                    }

                    qcs.insert(vote.justify_qc);

                    let (_bh, map) = vote_outcomes
                        .entry(vote.leaf_commitment)
                        .or_insert_with(|| (vote.block_commitment, BTreeMap::new()));
                    let value = map.insert(
                        vote.signature.0.clone(),
                        (vote.signature.1.clone(), vote.vote_token),
                    );
                    // let valid_signatures = map;

                    // TODO ed - this is repeated code from validate_qc, but should clean itself up once we implement I for Vote
                    // let mut signature_map: Signatures<I> = BTreeMap::new();
                    // TODO ed - there is a better way to do this, but it should be gone once I is impled for Vote


                    

                    // for signature in valid_signatures.clone() {
                    // let decoded_vote_token:VoteTokenType = bincode_opts().deserialize(&vote.vote_token).unwrap();
                    signature_map.insert(vote.signature.0.clone(), (vote.signature.1.clone(), vote_token.clone()));
                    stake_casted += u64::from(vote_token.vote_count());
                    // }

                    // TODO ed - current validated_stake rechecks that all votes are valid, which isn't necessary here
                    // let stake_casted = self.api.validated_stake(
                    //     vote.leaf_commitment,
                    //     self.cur_view,
                    //     signature_map,
                    // );

                    // let decoded_vote_token =
                    //     bincode_opts().deserialize(&value.unwrap().1).unwrap();

                    // stake_casted += decoded_vote_token.count();

                    error!("Stake casted is: {}", stake_casted);
                    error!("Next leader vote count duration is {:?}", Instant::now() - start);
                    if stake_casted >= u64::from(threshold) {
                        let stake_casted_validated = self.api.validated_stake(
                            vote.leaf_commitment,
                            self.cur_view,
                            signature_map,
                        );
                        if (stake_casted != stake_casted_validated) {
                            error!("Validated stake and unvalidated stake do NOT MATCH");
                        }
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
                        error!("Stake matches threshold!");
                        return qc;
                    }
                }
                ConsensusMessage::NextViewInterrupt(_view_number) => {
                    self.api.send_next_leader_timeout(self.cur_view).await;
                    error!("Leader has received a next view interrupt");
                    break;
                }
                ConsensusMessage::Proposal(_p) => {
                    warn!("The next leader has received an unexpected proposal!");
                }
            }
        }
        error!("Ending next leader round without enough stake, with number of votes: {}", num_votes);
        qcs.into_iter().max_by_key(|qc| qc.view_number).unwrap()
    }
}
