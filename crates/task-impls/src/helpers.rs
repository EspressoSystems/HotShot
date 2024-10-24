// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use core::time::Duration;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_broadcast::{Receiver, SendError, Sender};
use async_compatibility_layer::art::{async_sleep, async_spawn, async_timeout};
use async_lock::RwLock;
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use chrono::Utc;
use committable::{Commitment, Committable};
use hotshot_task::dependency::{Dependency, EventDependency};
use hotshot_types::{
    consensus::{ConsensusUpgradableReadLockGuard, OuterConsensus},
    data::{Leaf, QuorumProposal, ViewChangeEvidence},
    event::{Event, EventType, LeafInfo},
    message::{Proposal, UpgradeLock},
    request_response::ProposalRequestPayload,
    simple_certificate::{QuorumCertificate, UpgradeCertificate},
    traits::{
        block_contents::BlockHeader,
        election::Membership,
        node_implementation::{ConsensusTime, NodeImplementation, NodeType, Versions},
        signature_key::SignatureKey,
        storage::Storage,
        BlockPayload, ValidatedState,
    },
    utils::{Terminator, View, ViewInner},
    vote::{Certificate, HasViewNumber},
};
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::instrument;
use utils::anytrace::*;

use crate::{
    events::HotShotEvent, quorum_proposal_recv::QuorumProposalRecvTaskState,
    request::REQUEST_TIMEOUT,
};

/// Trigger a request to the network for a proposal for a view and wait for the response or timeout.
#[instrument(skip_all)]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn fetch_proposal<TYPES: NodeType, V: Versions>(
    view_number: TYPES::View,
    event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
    event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
    quorum_membership: Arc<TYPES::Membership>,
    consensus: OuterConsensus<TYPES>,
    sender_public_key: TYPES::SignatureKey,
    sender_private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
    upgrade_lock: &UpgradeLock<TYPES, V>,
) -> Result<Leaf<TYPES>> {
    // We need to be able to sign this request before submitting it to the network. Compute the
    // payload first.
    let signed_proposal_request = ProposalRequestPayload {
        view_number,
        key: sender_public_key,
    };

    // Finally, compute the signature for the payload.
    let signature = TYPES::SignatureKey::sign(
        &sender_private_key,
        signed_proposal_request.commit().as_ref(),
    )
    .wrap()
    .context(error!("Failed to sign proposal. This should never happen."))?;

    // First, broadcast that we need a proposal to the current leader
    broadcast_event(
        HotShotEvent::QuorumProposalRequestSend(signed_proposal_request, signature).into(),
        &event_sender,
    )
    .await;

    let mem = Arc::clone(&quorum_membership);
    let current_epoch = consensus.read().await.cur_epoch();
    // Make a background task to await the arrival of the event data.
    let Ok(Some(proposal)) =
        // We want to explicitly timeout here so we aren't waiting around for the data.
        async_timeout(REQUEST_TIMEOUT, async move {
            // We want to iterate until the proposal is not None, or until we reach the timeout.
            let mut proposal = None;
            while proposal.is_none() {
                // First, capture the output from the event dependency
                let event = EventDependency::new(
                    event_receiver.clone(),
                    Box::new(move |event| {
                        let event = event.as_ref();
                        if let HotShotEvent::QuorumProposalResponseRecv(
                            quorum_proposal,
                        ) = event
                        {
                            quorum_proposal.data.view_number() == view_number
                        } else {
                            false
                        }
                    }),
                )
                    .completed()
                    .await;

                // Then, if it's `Some`, make sure that the data is correct
                if let Some(hs_event) = event.as_ref() {
                    if let HotShotEvent::QuorumProposalResponseRecv(quorum_proposal) =
                        hs_event.as_ref()
                    {
                        // Make sure that the quorum_proposal is valid
                        if quorum_proposal.validate_signature(&mem, current_epoch, upgrade_lock).await.is_ok() {
                            proposal = Some(quorum_proposal.clone());
                        }

                    }
                }
            }

            proposal
        })
        .await
    else {
        bail!("Request for proposal failed");
    };

    let view_number = proposal.data.view_number();
    let justify_qc = proposal.data.justify_qc.clone();

    if !justify_qc
        .is_valid_cert(quorum_membership.as_ref(), current_epoch, upgrade_lock)
        .await
    {
        bail!("Invalid justify_qc in proposal for view {}", *view_number);
    }
    let mut consensus_write = consensus.write().await;
    let leaf = Leaf::from_quorum_proposal(&proposal.data);
    let state = Arc::new(
        <TYPES::ValidatedState as ValidatedState<TYPES>>::from_header(&proposal.data.block_header),
    );

    let view = View {
        view_inner: ViewInner::Leaf {
            leaf: leaf.commit(upgrade_lock).await,
            state,
            delta: None,
        },
    };
    if let Err(e) = consensus_write.update_validated_state_map(view_number, view.clone()) {
        tracing::trace!("{e:?}");
    }

    consensus_write
        .update_saved_leaves(leaf.clone(), upgrade_lock)
        .await;
    broadcast_event(
        HotShotEvent::ValidatedStateUpdated(view_number, view).into(),
        &event_sender,
    )
    .await;
    Ok(leaf)
}

/// Helper type to give names and to the output values of the leaf chain traversal operation.
#[derive(Debug)]
pub struct LeafChainTraversalOutcome<TYPES: NodeType> {
    /// The new locked view obtained from a 2 chain starting from the proposal's parent.
    pub new_locked_view_number: Option<TYPES::View>,

    /// The new decided view obtained from a 3 chain starting from the proposal's parent.
    pub new_decided_view_number: Option<TYPES::View>,

    /// The qc for the decided chain.
    pub new_decide_qc: Option<QuorumCertificate<TYPES>>,

    /// The decided leaves with corresponding validated state and VID info.
    pub leaf_views: Vec<LeafInfo<TYPES>>,

    /// The decided leaves.
    pub leaves_decided: Vec<Leaf<TYPES>>,

    /// The transactions in the block payload for each leaf.
    pub included_txns: Option<HashSet<Commitment<<TYPES as NodeType>::Transaction>>>,

    /// The most recent upgrade certificate from one of the leaves.
    pub decided_upgrade_cert: Option<UpgradeCertificate<TYPES>>,
}

/// We need Default to be implemented because the leaf ascension has very few failure branches,
/// and when they *do* happen, we still return intermediate states. Default makes the burden
/// of filling values easier.
impl<TYPES: NodeType + Default> Default for LeafChainTraversalOutcome<TYPES> {
    /// The default method for this type is to set all of the returned values to `None`.
    fn default() -> Self {
        Self {
            new_locked_view_number: None,
            new_decided_view_number: None,
            new_decide_qc: None,
            leaf_views: Vec::new(),
            leaves_decided: Vec::new(),
            included_txns: None,
            decided_upgrade_cert: None,
        }
    }
}

/// Ascends the leaf chain by traversing through the parent commitments of the proposal. We begin
/// by obtaining the parent view, and if we are in a chain (i.e. the next view from the parent is
/// one view newer), then we begin attempting to form the chain. This is a direct impl from
/// [HotStuff](https://arxiv.org/pdf/1803.05069) section 5:
///
/// > When a node b* carries a QC that refers to a direct parent, i.e., b*.justify.node = b*.parent,
/// we say that it forms a One-Chain. Denote by b'' = b*.justify.node. Node b* forms a Two-Chain,
/// if in addition to forming a One-Chain, b''.justify.node = b''.parent.
/// It forms a Three-Chain, if b'' forms a Two-Chain.
///
/// We follow this exact logic to determine if we are able to reach a commit and a decide. A commit
/// is reached when we have a two chain, and a decide is reached when we have a three chain.
///
/// # Example
/// Suppose we have a decide for view 1, and we then move on to get undecided views 2, 3, and 4. Further,
/// suppose that our *next* proposal is for view 5, but this leader did not see info for view 4, so the
/// justify qc of the proposal points to view 3. This is fine, and the undecided chain now becomes
/// 2-3-5.
///
/// Assuming we continue with honest leaders, we then eventually could get a chain like: 2-3-5-6-7-8. This
/// will prompt a decide event to occur (this code), where the `proposal` is for view 8. Now, since the
/// lowest value in the 3-chain here would be 5 (excluding 8 since we only walk the parents), we begin at
/// the first link in the chain, and walk back through all undecided views, making our new anchor view 5,
/// and out new locked view will be 6.
///
/// Upon receipt then of a proposal for view 9, assuming it is valid, this entire process will repeat, and
/// the anchor view will be set to view 6, with the locked view as view 7.
pub async fn decide_from_proposal<TYPES: NodeType>(
    proposal: &QuorumProposal<TYPES>,
    consensus: OuterConsensus<TYPES>,
    existing_upgrade_cert: Arc<RwLock<Option<UpgradeCertificate<TYPES>>>>,
    public_key: &TYPES::SignatureKey,
) -> LeafChainTraversalOutcome<TYPES> {
    let consensus_reader = consensus.read().await;
    let existing_upgrade_cert_reader = existing_upgrade_cert.read().await;
    let view_number = proposal.view_number();
    let parent_view_number = proposal.justify_qc.view_number();
    let old_anchor_view = consensus_reader.last_decided_view();

    let mut last_view_number_visited = view_number;
    let mut current_chain_length = 0usize;
    let mut res = LeafChainTraversalOutcome::default();

    if let Err(e) = consensus_reader.visit_leaf_ancestors(
        parent_view_number,
        Terminator::Exclusive(old_anchor_view),
        true,
        |leaf, state, delta| {
            // This is the core paper logic. We're implementing the chain in chained hotstuff.
            if res.new_decided_view_number.is_none() {
                // If the last view number is the child of the leaf we've moved to...
                if last_view_number_visited == leaf.view_number() + 1 {
                    last_view_number_visited = leaf.view_number();

                    // The chain grows by one
                    current_chain_length += 1;

                    // We emit a locked view when the chain length is 2
                    if current_chain_length == 2 {
                        res.new_locked_view_number = Some(leaf.view_number());
                        // The next leaf in the chain, if there is one, is decided, so this
                        // leaf's justify_qc would become the QC for the decided chain.
                        res.new_decide_qc = Some(leaf.justify_qc().clone());
                    } else if current_chain_length == 3 {
                        // And we decide when the chain length is 3.
                        res.new_decided_view_number = Some(leaf.view_number());
                    }
                } else {
                    // There isn't a new chain extension available, so we signal to the callback
                    // owner that we can exit for now.
                    return false;
                }
            }

            // Now, if we *have* reached a decide, we need to do some state updates.
            if let Some(new_decided_view) = res.new_decided_view_number {
                // First, get a mutable reference to the provided leaf.
                let mut leaf = leaf.clone();

                // Update the metrics
                if leaf.view_number() == new_decided_view {
                    consensus_reader
                        .metrics
                        .last_synced_block_height
                        .set(usize::try_from(leaf.height()).unwrap_or(0));
                }

                // Check if there's a new upgrade certificate available.
                if let Some(cert) = leaf.upgrade_certificate() {
                    if leaf.upgrade_certificate() != *existing_upgrade_cert_reader {
                        if cert.data.decide_by < view_number {
                            tracing::warn!(
                                "Failed to decide an upgrade certificate in time. Ignoring."
                            );
                        } else {
                            tracing::info!("Reached decide on upgrade certificate: {:?}", cert);
                            res.decided_upgrade_cert = Some(cert.clone());
                        }
                    }
                }
                // If the block payload is available for this leaf, include it in
                // the leaf chain that we send to the client.
                if let Some(encoded_txns) =
                    consensus_reader.saved_payloads().get(&leaf.view_number())
                {
                    let payload =
                        BlockPayload::from_bytes(encoded_txns, leaf.block_header().metadata());

                    leaf.fill_block_payload_unchecked(payload);
                }

                // Get the VID share at the leaf's view number, corresponding to our key
                // (if one exists)
                let vid_share = consensus_reader
                    .vid_shares()
                    .get(&leaf.view_number())
                    .unwrap_or(&HashMap::new())
                    .get(public_key)
                    .cloned()
                    .map(|prop| prop.data);

                // Add our data into a new `LeafInfo`
                res.leaf_views.push(LeafInfo::new(
                    leaf.clone(),
                    Arc::clone(&state),
                    delta.clone(),
                    vid_share,
                ));
                res.leaves_decided.push(leaf.clone());
                if let Some(ref payload) = leaf.block_payload() {
                    res.included_txns = Some(
                        payload
                            .transaction_commitments(leaf.block_header().metadata())
                            .into_iter()
                            .collect::<HashSet<_>>(),
                    );
                }
            }
            true
        },
    ) {
        tracing::debug!("Leaf ascension failed; error={e}");
    }

    res
}

/// Gets the parent leaf and state from the parent of a proposal, returning an [`utils::anytrace::Error`] if not.
#[instrument(skip_all)]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn parent_leaf_and_state<TYPES: NodeType, V: Versions>(
    next_proposal_view_number: TYPES::View,
    event_sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    event_receiver: &Receiver<Arc<HotShotEvent<TYPES>>>,
    quorum_membership: Arc<TYPES::Membership>,
    public_key: TYPES::SignatureKey,
    private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
    consensus: OuterConsensus<TYPES>,
    upgrade_lock: &UpgradeLock<TYPES, V>,
) -> Result<(Leaf<TYPES>, Arc<<TYPES as NodeType>::ValidatedState>)> {
    let current_epoch = consensus.read().await.cur_epoch();
    ensure!(
        quorum_membership.leader(next_proposal_view_number, current_epoch)? == public_key,
        info!(
            "Somehow we formed a QC but are not the leader for the next view {:?}",
            next_proposal_view_number
        )
    );
    let parent_view_number = consensus.read().await.high_qc().view_number();
    if !consensus
        .read()
        .await
        .validated_state_map()
        .contains_key(&parent_view_number)
    {
        let _ = fetch_proposal(
            parent_view_number,
            event_sender.clone(),
            event_receiver.clone(),
            quorum_membership,
            consensus.clone(),
            public_key.clone(),
            private_key.clone(),
            upgrade_lock,
        )
        .await
        .context(info!("Failed to fetch proposal"))?;
    }
    let consensus_reader = consensus.read().await;
    let parent_view_number = consensus_reader.high_qc().view_number();
    let parent_view = consensus_reader.validated_state_map().get(&parent_view_number).context(
        debug!("Couldn't find parent view in state map, waiting for replica to see proposal; parent_view_number: {}", *parent_view_number)
    )?;

    let (leaf_commitment, state) = parent_view.leaf_and_state().context(
        info!("Parent of high QC points to a view without a proposal; parent_view_number: {parent_view_number:?}, parent_view {parent_view:?}")
    )?;

    if leaf_commitment != consensus_reader.high_qc().data().leaf_commit {
        // NOTE: This happens on the genesis block
        tracing::debug!(
            "They don't equal: {:?}   {:?}",
            leaf_commitment,
            consensus_reader.high_qc().data().leaf_commit
        );
    }

    let leaf = consensus_reader
        .saved_leaves()
        .get(&leaf_commitment)
        .context(info!("Failed to find high QC of parent"))?;

    Ok((leaf.clone(), Arc::clone(state)))
}

/// Validate the state and safety and liveness of a proposal then emit
/// a `QuorumProposalValidated` event.
///
///
/// # Errors
/// If any validation or state update fails.
#[allow(clippy::too_many_lines)]
#[instrument(skip_all, fields(id = task_state.id, view = *proposal.data.view_number()))]
pub async fn validate_proposal_safety_and_liveness<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    V: Versions,
>(
    proposal: Proposal<TYPES, QuorumProposal<TYPES>>,
    parent_leaf: Leaf<TYPES>,
    task_state: &mut QuorumProposalRecvTaskState<TYPES, I, V>,
    event_stream: Sender<Arc<HotShotEvent<TYPES>>>,
    sender: TYPES::SignatureKey,
) -> Result<()> {
    let view_number = proposal.data.view_number();

    let proposed_leaf = Leaf::from_quorum_proposal(&proposal.data);
    ensure!(
        proposed_leaf.parent_commitment() == parent_leaf.commit(&task_state.upgrade_lock).await,
        "Proposed leaf does not extend the parent leaf."
    );

    let state = Arc::new(
        <TYPES::ValidatedState as ValidatedState<TYPES>>::from_header(&proposal.data.block_header),
    );
    let view = View {
        view_inner: ViewInner::Leaf {
            leaf: proposed_leaf.commit(&task_state.upgrade_lock).await,
            state,
            delta: None, // May be updated to `Some` in the vote task.
        },
    };

    {
        let mut consensus_write = task_state.consensus.write().await;
        if let Err(e) = consensus_write.update_validated_state_map(view_number, view.clone()) {
            tracing::trace!("{e:?}");
        }
        consensus_write
            .update_saved_leaves(proposed_leaf.clone(), &task_state.upgrade_lock)
            .await;

        // Update our internal storage of the proposal. The proposal is valid, so
        // we swallow this error and just log if it occurs.
        if let Err(e) = consensus_write.update_proposed_view(proposal.clone()) {
            tracing::debug!("Internal proposal update failed; error = {e:#}");
        };
    }

    // Broadcast that we've updated our consensus state so that other tasks know it's safe to grab.
    broadcast_event(
        Arc::new(HotShotEvent::ValidatedStateUpdated(view_number, view)),
        &event_stream,
    )
    .await;

    let current_epoch = task_state.cur_epoch;
    UpgradeCertificate::validate(
        &proposal.data.upgrade_certificate,
        &task_state.quorum_membership,
        current_epoch,
        &task_state.upgrade_lock,
    )
    .await?;

    // Validate that the upgrade certificate is re-attached, if we saw one on the parent
    proposed_leaf
        .extends_upgrade(
            &parent_leaf,
            &task_state.upgrade_lock.decided_upgrade_certificate,
        )
        .await?;

    let justify_qc = proposal.data.justify_qc.clone();
    // Create a positive vote if either liveness or safety check
    // passes.

    // Liveness check.
    {
        let read_consensus = task_state.consensus.read().await;
        let liveness_check = justify_qc.view_number() > read_consensus.locked_view();

        // Safety check.
        // Check if proposal extends from the locked leaf.
        let outcome = read_consensus.visit_leaf_ancestors(
            justify_qc.view_number(),
            Terminator::Inclusive(read_consensus.locked_view()),
            false,
            |leaf, _, _| {
                // if leaf view no == locked view no then we're done, report success by
                // returning true
                leaf.view_number() != read_consensus.locked_view()
            },
        );
        let safety_check = outcome.is_ok();

        ensure!(safety_check || liveness_check, {
            if let Err(e) = outcome {
                broadcast_event(
                    Event {
                        view_number,
                        event: EventType::Error { error: Arc::new(e) },
                    },
                    &task_state.output_event_stream,
                )
                .await;
            }

            error!("Failed safety and liveness check \n High QC is {:?}  Proposal QC is {:?}  Locked view is {:?}", read_consensus.high_qc(), proposal.data.clone(), read_consensus.locked_view())
        });
    }

    // Update our persistent storage of the proposal. If we cannot store the proposal reutrn
    // and error so we don't vote
    task_state
        .storage
        .write()
        .await
        .append_proposal(&proposal)
        .await
        .wrap()
        .context(error!("Failed to append proposal in storage!"))?;

    // We accept the proposal, notify the application layer
    broadcast_event(
        Event {
            view_number,
            event: EventType::QuorumProposal {
                proposal: proposal.clone(),
                sender,
            },
        },
        &task_state.output_event_stream,
    )
    .await;

    // Notify other tasks
    broadcast_event(
        Arc::new(HotShotEvent::QuorumProposalValidated(
            proposal.data.clone(),
            parent_leaf,
        )),
        &event_stream,
    )
    .await;

    Ok(())
}

/// Validates, from a given `proposal` that the view that it is being submitted for is valid when
/// compared to `cur_view` which is the highest proposed view (so far) for the caller. If the proposal
/// is for a view that's later than expected, that the proposal includes a timeout or view sync certificate.
///
/// # Errors
/// If any validation or view number check fails.
pub async fn validate_proposal_view_and_certs<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    V: Versions,
>(
    proposal: &Proposal<TYPES, QuorumProposal<TYPES>>,
    task_state: &mut QuorumProposalRecvTaskState<TYPES, I, V>,
) -> Result<()> {
    let view = proposal.data.view_number();
    ensure!(
        view >= task_state.cur_view,
        "Proposal is from an older view {:?}",
        proposal.data.clone()
    );

    // Validate the proposal's signature. This should also catch if the leaf_commitment does not equal our calculated parent commitment
    proposal
        .validate_signature(
            &task_state.quorum_membership,
            task_state.cur_epoch,
            &task_state.upgrade_lock,
        )
        .await?;

    // Verify a timeout certificate OR a view sync certificate exists and is valid.
    if proposal.data.justify_qc.view_number() != view - 1 {
        let received_proposal_cert =
            proposal.data.proposal_certificate.clone().context(debug!(
                "Quorum proposal for view {} needed a timeout or view sync certificate, but did not have one",
                *view
        ))?;

        match received_proposal_cert {
            ViewChangeEvidence::Timeout(timeout_cert) => {
                ensure!(
                    timeout_cert.data().view == view - 1,
                    "Timeout certificate for view {} was not for the immediately preceding view",
                    *view
                );
                ensure!(
                    timeout_cert
                        .is_valid_cert(
                            task_state.timeout_membership.as_ref(),
                            task_state.cur_epoch,
                            &task_state.upgrade_lock
                        )
                        .await,
                    "Timeout certificate for view {} was invalid",
                    *view
                );
            }
            ViewChangeEvidence::ViewSync(view_sync_cert) => {
                ensure!(
                    view_sync_cert.view_number == view,
                    "View sync cert view number {:?} does not match proposal view number {:?}",
                    view_sync_cert.view_number,
                    view
                );

                // View sync certs must also be valid.
                ensure!(
                    view_sync_cert
                        .is_valid_cert(
                            task_state.quorum_membership.as_ref(),
                            task_state.cur_epoch,
                            &task_state.upgrade_lock
                        )
                        .await,
                    "Invalid view sync finalize cert provided"
                );
            }
        }
    }

    // Validate the upgrade certificate -- this is just a signature validation.
    // Note that we don't do anything with the certificate directly if this passes; it eventually gets stored as part of the leaf if nothing goes wrong.
    UpgradeCertificate::validate(
        &proposal.data.upgrade_certificate,
        &task_state.quorum_membership,
        task_state.cur_epoch,
        &task_state.upgrade_lock,
    )
    .await?;

    Ok(())
}

/// Update the view if it actually changed, takes a mutable reference to the `cur_view` and the
/// `timeout_task` which are updated during the operation of the function.
///
/// # Errors
/// Returns an [`utils::anytrace::Error`] when the new view is not greater than the current view.
pub(crate) async fn update_view<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions>(
    new_view: TYPES::View,
    event_stream: &Sender<Arc<HotShotEvent<TYPES>>>,
    task_state: &mut QuorumProposalRecvTaskState<TYPES, I, V>,
) -> Result<()> {
    ensure!(
        new_view > task_state.cur_view,
        "New view is not greater than our current view"
    );

    let is_old_view_leader = task_state
        .quorum_membership
        .leader(task_state.cur_view, task_state.cur_epoch)?
        == task_state.public_key;
    let old_view = task_state.cur_view;

    tracing::debug!("Updating view from {} to {}", *old_view, *new_view);

    if *old_view / 100 != *new_view / 100 {
        tracing::info!("Progress: entered view {:>6}", *new_view);
    }

    task_state.cur_view = new_view;

    // The next view is just the current view + 1
    let next_view = task_state.cur_view + 1;

    futures::join! {
        broadcast_event(Arc::new(HotShotEvent::ViewChange(new_view)), event_stream),
        broadcast_event(
            Event {
                view_number: old_view,
                event: EventType::ViewFinished {
                    view_number: old_view,
                },
            },
            &task_state.output_event_stream,
        )
    };

    // Spawn a timeout task if we did actually update view
    let new_timeout_task = async_spawn({
        let stream = event_stream.clone();
        // Nuance: We timeout on the view + 1 here because that means that we have
        // not seen evidence to transition to this new view
        let view_number = next_view;
        let timeout = Duration::from_millis(task_state.timeout);
        async move {
            async_sleep(timeout).await;
            broadcast_event(
                Arc::new(HotShotEvent::Timeout(TYPES::View::new(*view_number))),
                &stream,
            )
            .await;
        }
    });

    // cancel the old timeout task
    cancel_task(std::mem::replace(
        &mut task_state.timeout_task,
        new_timeout_task,
    ))
    .await;

    let consensus = task_state.consensus.upgradable_read().await;
    consensus
        .metrics
        .current_view
        .set(usize::try_from(task_state.cur_view.u64()).unwrap());
    let new_view_time = Utc::now().timestamp();
    if is_old_view_leader {
        #[allow(clippy::cast_precision_loss)]
        consensus
            .metrics
            .view_duration_as_leader
            .add_point((new_view_time - task_state.cur_view_time) as f64);
    }
    task_state.cur_view_time = new_view_time;

    // Do the comparison before the subtraction to avoid potential overflow, since
    // `last_decided_view` may be greater than `cur_view` if the node is catching up.
    if usize::try_from(task_state.cur_view.u64()).unwrap()
        > usize::try_from(consensus.last_decided_view().u64()).unwrap()
    {
        consensus.metrics.number_of_views_since_last_decide.set(
            usize::try_from(task_state.cur_view.u64()).unwrap()
                - usize::try_from(consensus.last_decided_view().u64()).unwrap(),
        );
    }
    let mut consensus = ConsensusUpgradableReadLockGuard::upgrade(consensus).await;
    if let Err(e) = consensus.update_view(new_view) {
        tracing::trace!("{e:?}");
    }
    tracing::trace!("View updated successfully");

    Ok(())
}

/// Cancel a task
pub async fn cancel_task<T>(task: JoinHandle<T>) {
    #[cfg(async_executor_impl = "async-std")]
    task.cancel().await;
    #[cfg(async_executor_impl = "tokio")]
    task.abort();
}

/// Helper function to send events and log errors
pub async fn broadcast_event<E: Clone + std::fmt::Debug>(event: E, sender: &Sender<E>) {
    match sender.broadcast_direct(event).await {
        Ok(None) => (),
        Ok(Some(overflowed)) => {
            tracing::error!(
                "Event sender queue overflow, Oldest event removed form queue: {:?}",
                overflowed
            );
        }
        Err(SendError(e)) => {
            tracing::warn!(
                "Event: {:?}\n Sending failed, event stream probably shutdown",
                e
            );
        }
    }
}
