//! Contains the [`DAMember`] struct used for the committee member step in the consensus algorithm
//! with DA committee, i.e. in the sequencing consensus.

use crate::{Consensus, ConsensusApi};
use async_compatibility_layer::channel::UnboundedReceiver;
use async_lock::{Mutex, RwLock};
use commit::Committable;
use hotshot_types::message::Message;
use hotshot_types::{
    certificate::QuorumCertificate,
    data::{DAProposal, SequencingLeaf},
    message::{ConsensusMessage, ProcessedConsensusMessage},
    traits::{
        election::{CommitteeExchangeType, ConsensusExchange},
        node_implementation::{CommitteeProposal, CommitteeVote, NodeImplementation, NodeType},
        signature_key::SignatureKey,
    },
    vote::DAVote,
};
use std::marker::PhantomData;
use std::sync::Arc;
use tracing::{error, info, instrument, warn};

/// This view's DA committee member.
#[derive(Debug, Clone)]
pub struct DAMember<
    A: ConsensusApi<TYPES, SequencingLeaf<TYPES>, I>,
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
> {
    /// ID of node.
    pub id: u64,
    /// Reference to consensus. DA committee member will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES, SequencingLeaf<TYPES>>>>,
    /// Channel for accepting leader proposals and timeouts messages.
    #[allow(clippy::type_complexity)]
    pub proposal_collection_chan:
        Arc<Mutex<UnboundedReceiver<ProcessedConsensusMessage<TYPES, I>>>>,
    /// View number this view is executing in.
    pub cur_view: TYPES::Time,
    /// The High QC.
    pub high_qc: QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,
    /// HotShot consensus API.
    pub api: A,

    /// the committee exchange
    pub exchange: Arc<I::CommitteeExchange>,

    /// needed for type checking
    pub _pd: PhantomData<I>,
}

impl<
        A: ConsensusApi<TYPES, SequencingLeaf<TYPES>, I>,
        TYPES: NodeType,
        I: NodeImplementation<TYPES, Leaf = SequencingLeaf<TYPES>>,
    > DAMember<A, TYPES, I>
where
    I::CommitteeExchange: ConsensusExchange<
            TYPES,
            I::Leaf,
            Message<TYPES, I>,
            Proposal = DAProposal<TYPES>,
            Vote = DAVote<TYPES, SequencingLeaf<TYPES>>,
        > + CommitteeExchangeType<TYPES, I::Leaf, Message<TYPES, I>>,
{
    /// DA committee member task that spins until a valid DA proposal can be signed or timeout is
    /// hit.
    #[instrument(skip_all, fields(id = self.id, view = *self.cur_view), name = "DA Member Task", level = "error")]
    #[allow(clippy::type_complexity)]
    async fn find_valid_msg<'a>(
        &self,
        view_leader_key: TYPES::SignatureKey,
    ) -> Option<TYPES::BlockType> {
        let lock = self.proposal_collection_chan.lock().await;
        let leaf = loop {
            let msg = lock.recv().await;
            info!("recv-ed message {:?}", msg.clone());
            if let Ok(msg) = msg {
                // If the message is for a different view number, skip it.
                if Into::<ConsensusMessage<_, _>>::into(msg.clone()).view_number() != self.cur_view
                {
                    continue;
                }
                match msg {
                    ProcessedConsensusMessage::DAProposal(p, sender) => {
                        if view_leader_key != sender {
                            continue;
                        }

                        let block_commitment = p.data.deltas.commit();
                        if !view_leader_key.validate(&p.signature, block_commitment.as_ref()) {
                            warn!(?p.signature, "Could not verify proposal.");
                            continue;
                        }

                        let vote_token = self.exchange.make_vote_token(self.cur_view);
                        match vote_token {
                            Err(e) => {
                                error!(
                                    "Failed to generate vote token for {:?} {:?}",
                                    self.cur_view, e
                                );
                            }
                            Ok(None) => {
                                info!("We were not chosen for DA committee on {:?}", self.cur_view);
                            }
                            Ok(Some(vote_token)) => {
                                info!("We were chosen for DA committee on {:?}", self.cur_view);

                                // Generate and send vote
                                let message = self.exchange.create_da_message(
                                    self.high_qc.commit(),
                                    block_commitment,
                                    self.cur_view,
                                    vote_token,
                                );

                                info!("Sending vote to the leader {:?}", message);

                                let consensus = self.consensus.read().await;
                                if self.api.send_direct_da_message::<CommitteeProposal<TYPES, I>, CommitteeVote<TYPES, I>>(sender, message).await.is_err() {
                                    consensus.metrics.failed_to_send_messages.add(1);
                                    warn!("Failed to send vote to the leader");
                                } else {
                                    consensus.metrics.outgoing_direct_messages.add(1);
                                }
                            }
                        }
                        break p.data.deltas;
                    }
                    ProcessedConsensusMessage::InternalTrigger(_trigger) => {
                        warn!("DA committee member receieved an internal trigger message. This is not what the member expects. Skipping.");
                        continue;
                    }
                    ProcessedConsensusMessage::Vote(_, _) => {
                        // Should only be for DA leader, never member.
                        warn!("DA committee member receieved a vote message. This is not what the member expects. Skipping.");
                        continue;
                    }
                    ProcessedConsensusMessage::DAVote(_, _) => {
                        // Should only be for DA leader, never member.
                        warn!("DA committee member receieved a vote message. This is not what the member expects. Skipping.");
                        continue;
                    }
                    ProcessedConsensusMessage::Proposal(_, _) => {
                        warn!("DA committee member receieved a Non DA Proposal message. This is not what the member expects. Skipping.");
                        continue;
                    }
                }
            }
            // fall through logic if we did not receive successfully from channel
            warn!("DA committee member did not receive successfully from channel.");
            return None;
        };
        Some(leaf)
    }

    /// Run one view of DA committee member.
    #[instrument(skip(self), fields(id = self.id, view = *self.cur_view), name = "DA Member Task", level = "error")]
    pub async fn run_view(self) -> QuorumCertificate<TYPES, SequencingLeaf<TYPES>> {
        info!("DA Committee Member task started!");
        let view_leader_key = self.exchange.get_leader(self.cur_view);

        let maybe_block = self.find_valid_msg(view_leader_key).await;

        let Some(_block) = maybe_block else {
            // We either timed out or for some reason could not accept a proposal.
            return self.high_qc;
        };

        self.high_qc
    }
}
