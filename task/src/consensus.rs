use crate::HandleEvent;

#[derive(Snafu, Debug)]
pub struct ConsensusTaskError {}
impl TaskErr for ConsensusTaskError {}

#[derive(Debug)]
pub struct SequencingConsensusTaskState {
    /// Reference to consensus. The replica will require a write lock on this.
    pub consensus: Arc<RwLock<Consensus<TYPES, SequencingLeaf<TYPES>>>>,
    /// Channel for accepting leader proposals and timeouts messages.
    #[allow(clippy::type_complexity)]
    pub proposal_collection_chan:
        Arc<Mutex<UnboundedReceiver<ProcessedSequencingMessage<TYPES, I>>>>,
    /// View number this view is executing in.
    pub cur_view: TYPES::Time,
    /// The High QC.
    pub high_qc: QuorumCertificate<TYPES, SequencingLeaf<TYPES>>,
    /// HotShot consensus API.
    pub api: A,

    /// the quorum exchange
    pub quorum_exchange: Arc<SequencingQuorumEx<TYPES, I>>,

    /// needed to typecheck
    pub _pd: PhantomData<I>,
}

impl SequencingConsensusTaskState {
    pub fn handle_event(&self, event: SequencingHotShotEvent) {
        match event {
            SequencingHotShotEvent::Shutdown => {}
            SequencingHotShotEvent::QuorumProposalRecv(proposal, sender) => {
                let view_leader_key = self.quorum_exchange.get_leader(self.cur_view);
                if view_leader_key != sender {
                    continue;
                }

                let mut valid_leaf = None;
                let vote_token =
                    self.quorum_exchange.make_vote_token(self.cur_view);
                match vote_token {
                    Err(e) => {
                        error!(
                            "Failed to generate vote token for {:?} {:?}",
                            self.cur_view, e
                        );
                    }
                    Ok(None) => {
                        info!(
                            "We were not chosen for consensus committee on {:?}",
                            self.cur_view
                        );
                    }
                    Ok(Some(vote_token)) => {
                        info!(
                            "We were chosen for consensus committee on {:?}",
                            self.cur_view
                        );

                        let message;

                        // Construct the leaf.
                        let justify_qc = p.data.justify_qc;
                        let parent = if justify_qc.is_genesis() {
                            self.genesis_leaf().await
                        } else {
                            consensus
                                .saved_leaves
                                .get(&justify_qc.leaf_commitment())
                                .cloned()
                        };
                        let Some(parent) = parent else {
                            warn!("Proposal's parent missing from storage");
                            return;
                        };
                        let parent_commitment = parent.commit();
                        let block_commitment = p.data.block_commitment;
                        let leaf = SequencingLeaf {
                            view_number: self.cur_view,
                            height: p.data.height,
                            justify_qc: justify_qc.clone(),
                            parent_commitment,
                            deltas: Right(p.data.block_commitment),
                            rejected: Vec::new(),
                            timestamp: time::OffsetDateTime::now_utc()
                                .unix_timestamp_nanos(),
                            proposer_id: sender.to_bytes(),
                        };
                        let justify_qc_commitment = justify_qc.commit();
                        let leaf_commitment = leaf.commit();

                        // Validate the `justify_qc`.
                        if !self
                            .quorum_exchange
                            .is_valid_cert(&justify_qc, parent_commitment)
                        {
                            invalid_qc = true;
                            warn!("Invalid justify_qc in proposal!.");
                            message = self.quorum_exchange.create_no_message(
                                justify_qc_commitment,
                                leaf_commitment,
                                self.cur_view,
                                vote_token,
                            );
                        }
                        // Validate the `height`.
                        else if leaf.height != parent.height + 1 {
                            invalid_qc = true;
                            warn!(
                                "Incorrect height in proposal (expected {}, got {})",
                                parent.height + 1,
                                leaf.height
                            );
                            message = self.quorum_exchange.create_no_message(
                                justify_qc_commitment,
                                leaf_commitment,
                                self.cur_view,
                                vote_token,
                            );
                        }
                        // Validate the DAC.
                        else if !self
                            .committee_exchange
                            .is_valid_cert(&p.data.dac, block_commitment)
                        {
                            warn!("Invalid DAC in proposal! Skipping proposal.");
                            message = self.quorum_exchange.create_no_message(
                                justify_qc_commitment,
                                leaf_commitment,
                                self.cur_view,
                                vote_token,
                            );
                        }
                        // Validate the signature.
                        else if !view_leader_key
                            .validate(&p.signature, leaf_commitment.as_ref())
                        {
                            warn!(?p.signature, "Could not verify proposal.");
                            message = self.quorum_exchange.create_no_message(
                                justify_qc_commitment,
                                leaf_commitment,
                                self.cur_view,
                                vote_token,
                            );
                        }
                        // Create a positive vote if either liveness or safety check
                        // passes.
                        else {
                            // Liveness check.
                            let liveness_check =
                                justify_qc.view_number > consensus.locked_view;

                            // Safety check.
                            // Check if proposal extends from the locked leaf.
                            let outcome = consensus.visit_leaf_ancestors(
                                justify_qc.view_number,
                                Terminator::Inclusive(consensus.locked_view),
                                false,
                                |leaf| {
                                    // if leaf view no == locked view no then we're done, report success by
                                    // returning true
                                    leaf.view_number != consensus.locked_view
                                },
                            );
                            let safety_check = outcome.is_ok();
                            if let Err(e) = outcome {
                                self.api
                                    .send_view_error(self.cur_view, Arc::new(e))
                                    .await;
                            }

                            // Skip if both saftey and liveness checks fail.
                            if !safety_check && !liveness_check {
                                warn!("Failed safety check and liveness check");
                                message = self.quorum_exchange.create_no_message(
                                    justify_qc_commitment,
                                    leaf_commitment,
                                    self.cur_view,
                                    vote_token,
                                );
                            } else {
                                // A valid leaf is found.
                                valid_leaf = Some(leaf);

                                // Generate a message with yes vote.
                                message = self.quorum_exchange.create_yes_message(
                                    justify_qc_commitment,
                                    leaf_commitment,
                                    self.cur_view,
                                    vote_token,
                                );
                            }
                        }

                        info!("Sending vote to next leader {:?}", message);
                        let next_leader =
                            self.quorum_exchange.get_leader(self.cur_view + 1);
                        if self
                            .api
                            .send_direct_message::<QuorumProposalType<TYPES, I>, QuorumVoteType<TYPES, I>>(next_leader, SequencingMessage(Left(message)))
                            .await
                            .is_err()
                        {
                            consensus.metrics.failed_to_send_messages.add(1);
                            warn!("Failed to send vote to next leader");
                        } else {
                            consensus.metrics.outgoing_direct_messages.add(1);
                        }
                    }
                }
            }
            SequencingHotShotEvent::DAProposalRecv(proposal, sender) => {}
            SequencingHotShotEvent::QuorumVoteRecv(vote, sender) => {}
            SequencingHotShotEvent::DAVoteRecv(vote, sender) => {}
            SequencingHotShotEvent::ViewSyncMessage => {}
            SequencingHotShotEvent::QuorumProposalSend => {}
            SequencingHotShotEvent::DAProposalSend => {}
            SequencingHotShotEvent::VoteSend => {}
            SequencingHotShotEvent::DAVoteSend => {}
            SequencingHotShotEvent::ViewChange => {}
            SequencingHotShotEvent::Timeout => {}
        }
    }
}

impl TS for ConsensusTaskState {}


pub type ConsensusTaskTypes =
    HSTWithEvent<ConsensusTaskError, SequencingHotShotEvent, ChannelStream<SequencingHotShotEvent>, ConsensusTaskState>;

static event_handle: HandleEvent<HSTT = ConsensusTaskTypes> =
    HandleEvent::<HSTT = ConsensusTaskTypes>(Arc::new(move |event, state| {
        async move {
            if let SequencingHotShotEvent::Shutdown = event {
                (Some(HotShotTaskCompleted::ShutDown), state)
            } else {
                state.handle_event(event);
                (None, state)
            }
        }
        .boxed()
    }));
