//! Contains the [`NextValidatingLeader`] struct used for the next leader step in the hotstuff consensus algorithm.

use crate::ConsensusMetrics;
use crate::ValidatingConsensusApi;
use async_compatibility_layer::channel::UnboundedReceiver;
use async_lock::Mutex;
use either::Either;
use either::{Left, Right};
use hotshot_types::data::ValidatingLeaf;
use hotshot_types::message::Message;
use hotshot_types::message::ProcessedGeneralConsensusMessage;
use hotshot_types::traits::election::ConsensusExchange;
use hotshot_types::traits::election::QuorumExchangeType;
use hotshot_types::traits::election::{Checked::Unchecked, VoteData};
use hotshot_types::traits::node_implementation::{
    NodeImplementation, NodeType, ValidatingExchangesType, ValidatingQuorumEx,
};
use hotshot_types::traits::signature_key::SignatureKey;
use hotshot_types::vote::VoteAccumulator;
use hotshot_types::{
    certificate::QuorumCertificate,
    message::{InternalTrigger, ValidatingMessage},
    traits::consensus_type::validating_consensus::ValidatingConsensus,
    vote::QuorumVote,
};
use std::marker::PhantomData;
use std::time::Instant;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tracing::{info, instrument, warn};

/// The next view's validating leader
#[derive(custom_debug::Debug, Clone)]
pub struct NextValidatingLeader<
    A: ValidatingConsensusApi<TYPES, ValidatingLeaf<TYPES>, I>,
    TYPES: NodeType<ConsensusType = ValidatingConsensus>,
    I: NodeImplementation<
        TYPES,
        Leaf = ValidatingLeaf<TYPES>,
        ConsensusMessage = ValidatingMessage<TYPES, I>,
    >,
> where
    I::Exchanges: ValidatingExchangesType<TYPES, Message<TYPES, I>>,
{
    /// id of node
    pub id: u64,
    /// generic_qc before starting this
    pub generic_qc: QuorumCertificate<TYPES, ValidatingLeaf<TYPES>>,
    /// channel through which the leader collects votes
    #[allow(clippy::type_complexity)]
    pub vote_collection_chan:
        Arc<Mutex<UnboundedReceiver<ProcessedGeneralConsensusMessage<TYPES, I>>>>,
    /// The view number we're running on
    pub cur_view: TYPES::Time,
    /// Limited access to the consensus protocol
    pub api: A,

    /// quorum exchange
    pub exchange: Arc<ValidatingQuorumEx<TYPES, I>>,
    /// Metrics for reporting stats
    #[debug(skip)]
    pub metrics: Arc<ConsensusMetrics>,

    /// needed to type check
    pub _pd: PhantomData<I>,
}

impl<
        A: ValidatingConsensusApi<TYPES, ValidatingLeaf<TYPES>, I>,
        TYPES: NodeType<ConsensusType = ValidatingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = ValidatingLeaf<TYPES>,
            ConsensusMessage = ValidatingMessage<TYPES, I>,
        >,
    > NextValidatingLeader<A, TYPES, I>
where
    I::Exchanges: ValidatingExchangesType<TYPES, Message<TYPES, I>>,
    ValidatingQuorumEx<TYPES, I>: ConsensusExchange<
        TYPES,
        ValidatingLeaf<TYPES>,
        Message<TYPES, I>,
        Certificate = QuorumCertificate<TYPES, ValidatingLeaf<TYPES>>,
        Commitment = ValidatingLeaf<TYPES>,
    >,
{
    /// Run one view of the next leader task
    /// # Panics
    /// While we are unwrapping, this function can logically never panic
    /// unless there is a bug in std
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "Next Validating ValidatingLeader Task", level = "error")]
    pub async fn run_view(self) -> QuorumCertificate<TYPES, ValidatingLeaf<TYPES>> {
        info!("Next validating leader task started!");

        let vote_collection_start = Instant::now();

        let mut qcs = HashSet::<QuorumCertificate<TYPES, ValidatingLeaf<TYPES>>>::new();
        qcs.insert(self.generic_qc.clone());

        let mut accumlator = VoteAccumulator {
            vote_outcomes: HashMap::new(),
            threshold: self.exchange.threshold(),
        };

        let lock = self.vote_collection_chan.lock().await;
        while let Ok(msg) = lock.recv().await {
            // If the message is for a different view number, skip it.
            if Into::<ValidatingMessage<_, _>>::into(msg.clone()).view_number() != self.cur_view {
                continue;
            }
            match msg {
                ProcessedGeneralConsensusMessage::Vote(vote_message, sender) => {
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
                ProcessedGeneralConsensusMessage::InternalTrigger(trigger) => match trigger {
                    InternalTrigger::Timeout(_) => {
                        self.api.send_next_leader_timeout(self.cur_view).await;
                        break;
                    }
                },
                ProcessedGeneralConsensusMessage::Proposal(_p, _sender) => {
                    warn!("The next leader has received an unexpected proposal!");
                }
            }
        }

        qcs.into_iter()
            .max_by_key(hotshot_types::traits::election::SignedCertificate::view_number)
            .unwrap()
    }
}
