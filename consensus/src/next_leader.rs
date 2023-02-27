//! Contains the [`NextValidatingLeader`] struct used for the next leader step in the hotstuff consensus algorithm.

use crate::ConsensusApi;
use crate::ConsensusMetrics;
use async_compatibility_layer::channel::UnboundedReceiver;
use async_lock::Mutex;
use either::Either;
use hotshot_types::certificate::CertificateAccumulator;
use hotshot_types::data::{ValidatingLeaf, ValidatingProposal};
use hotshot_types::message::ProcessedConsensusMessage;
use hotshot_types::traits::election::ConsensusExchange;
use hotshot_types::traits::election::SignedCertificate;
use hotshot_types::traits::election::{Checked::Unchecked, VoteData};
use hotshot_types::traits::node_implementation::NodeType;
use hotshot_types::traits::signature_key::SignatureKey;
use hotshot_types::{
    certificate::QuorumCertificate,
    message::{ConsensusMessage, InternalTrigger, QuorumVote},
};
use std::time::Instant;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tracing::{error, instrument, warn};

/// The next view's validating leader
#[derive(custom_debug::Debug, Clone)]
pub struct NextValidatingLeader<
    A: ConsensusApi<
        TYPES,
        ValidatingLeaf<TYPES>,
        ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
    >,
    EXCHANGE: ConsensusExchange<TYPES, ValidatingLeaf<TYPES>>,
    TYPES: NodeType,
> {
    /// id of node
    pub id: u64,
    /// generic_qc before starting this
    pub generic_qc: EXCHANGE::Certificate,
    /// channel through which the leader collects votes
    #[allow(clippy::type_complexity)]
    pub vote_collection_chan: Arc<
        Mutex<
            UnboundedReceiver<
                ProcessedConsensusMessage<
                    TYPES,
                    ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
                    QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
                >,
            >,
        >,
    >,
    /// The view number we're running on
    pub cur_view: TYPES::Time,
    /// Limited access to the consensus protocol
    pub api: A,

    pub exchange: EXCHANGE,
    /// Metrics for reporting stats
    #[debug(skip)]
    pub metrics: Arc<ConsensusMetrics>,
}

impl<
        A: ConsensusApi<
            TYPES,
            ValidatingLeaf<TYPES>,
            ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
            QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        >,
        EXCHANGE: ConsensusExchange<TYPES, ValidatingLeaf<TYPES>>,
        TYPES: NodeType,
    > NextValidatingLeader<A, EXCHANGE, TYPES>
{
    /// Run one view of the next leader task
    /// # Panics
    /// While we are unwrapping, this function can logically never panic
    /// unless there is a bug in std
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Next Validating ValidatingLeader Task", level = "error")]
    pub async fn run_view(self) -> EXCHANGE::Certificate {
        error!("Next validating leader task started!");

        let vote_collection_start = Instant::now();

        let mut qcs = HashSet::<EXCHANGE::Certificate>::new();
        qcs.insert(self.generic_qc.clone());

        let mut accumlator = CertificateAccumulator {
            vote_outcomes: HashMap::new(),
            threshold: self.api.threshold(),
        };

        let lock = self.vote_collection_chan.lock().await;
        while let Ok(msg) = lock.recv().await {
            // If the message is for a different view number, skip it.
            if Into::<ConsensusMessage<_, _, _>>::into(msg.clone()).view_number() != self.cur_view {
                continue;
            }
            match msg {
                ProcessedConsensusMessage::Vote(vote_message, sender) => {
                    match vote_message {
                        QuorumVote::Yes(vote) => {
                            if vote.signature.0
                                != <TYPES::SignatureKey as SignatureKey>::to_bytes(&sender)
                            {
                                continue;
                            }
                            match self.exchange.accumulate_vote(
                                &vote.signature.0,
                                &vote.signature.1,
                                vote.leaf_commitment,
                                vote.vote_token.clone(),
                                self.cur_view,
                                accumlator,
                            ) {
                                Either::Left(acc) => {
                                    accumlator = acc;
                                }
                                Either::Right(qc) => {
                                    self.metrics
                                        .vote_validate_duration
                                        .add_point(vote_collection_start.elapsed().as_secs_f64());
                                    return qc;
                                }
                            }
                            // If the signature on the vote is invalid, assume it's sent by
                            // byzantine node and ignore.
                            if !self.exchange.is_valid_vote(
                                &vote.signature.0,
                                &vote.signature.1,
                                VoteData::Yes(vote.leaf_commitment),
                                vote.current_view,
                                // Ignoring deserialization errors below since we are getting rid of it soon
                                Unchecked(vote.vote_token.clone()),
                            ) {
                                continue;
                            }
                        }
                        QuorumVote::Timeout(vote) => {
                            qcs.insert(vote.justify_qc);
                        }
                        QuorumVote::No(_) => {
                            warn!("The next leader has received an unexpected vote!");
                        }
                    }
                }
                ProcessedConsensusMessage::InternalTrigger(trigger) => match trigger {
                    InternalTrigger::Timeout(_) => {
                        self.api.send_next_leader_timeout(self.cur_view).await;
                        break;
                    }
                },
                ProcessedConsensusMessage::Proposal(_p, _sender) => {
                    warn!("The next leader has received an unexpected proposal!");
                }
            }
        }

        qcs.into_iter().max_by_key(|qc| qc.view_number()).unwrap()
    }
}
