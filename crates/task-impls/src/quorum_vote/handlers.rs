// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{collections::btree_map::Entry, sync::Arc};

use async_broadcast::{InactiveReceiver, Sender};
use async_lock::RwLock;
use chrono::Utc;
use committable::Committable;
use hotshot_types::{
    consensus::OuterConsensus,
    data::{Leaf2, QuorumProposal2, VidDisperseShare2},
    drb::{compute_drb_result, DrbResult},
    event::{Event, EventType, LeafInfo},
    message::{Proposal, UpgradeLock},
    simple_vote::{QuorumData2, QuorumVote2},
    traits::{
        block_contents::BlockHeader,
        election::Membership,
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
        signature_key::SignatureKey,
        storage::Storage,
        ValidatedState,
    },
    utils::{epoch_from_block_number, is_epoch_root, is_last_block_in_epoch},
    vote::HasViewNumber,
};
use tokio::spawn;
use tracing::instrument;
use utils::anytrace::*;
use vbs::version::StaticVersionType;

use super::QuorumVoteTaskState;
use crate::{
    events::HotShotEvent,
    helpers::{
        broadcast_event, decide_from_proposal, decide_from_proposal_2, fetch_proposal,
        LeafChainTraversalOutcome,
    },
    quorum_vote::Versions,
};

/// Store the DRB result from the quorum proposal.
///
/// Returns an error if receiving an inconsistent result.
async fn store_received_drb_result<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions>(
    epoch_number: TYPES::Epoch,
    drb_result: DrbResult,
    task_state: &mut QuorumVoteTaskState<TYPES, I, V>,
) -> Result<()> {
    let mut exist = false;
    if let Some(stored_result) = task_state
        .consensus
        .read()
        .await
        .drb_seeds_and_results
        .results
        .get(&epoch_number)
    {
        if drb_result == *stored_result {
            return Ok(());
        }
        exist = true;
    }

    // If there exists an inconsistent result, remove it.
    if exist {
        task_state
            .consensus
            .write()
            .await
            .drb_seeds_and_results
            .results
            .remove(&epoch_number);
        bail!("Inconsistent result with the storage.");
    }
    // Otherwise, store the result.
    else {
        task_state
            .consensus
            .write()
            .await
            .drb_seeds_and_results
            .results
            .insert(epoch_number, drb_result);
        Ok(())
    }
}

/// Store the DRB result from the computation task to the shared `results` table.
///
/// Returns the result if it exists.
async fn store_and_get_computed_drb_result<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    V: Versions,
>(
    epoch_number: TYPES::Epoch,
    task_state: &mut QuorumVoteTaskState<TYPES, I, V>,
) -> Result<DrbResult> {
    // Return the result if it's already in the table.
    if let Some(computed_result) = task_state
        .consensus
        .read()
        .await
        .drb_seeds_and_results
        .results
        .get(&epoch_number)
    {
        return Ok(*computed_result);
    }

    let (task_epoch, computation) =
        (&mut task_state.drb_computation).context(warn!("DRB computation task doesn't exist."))?;

    ensure!(
        *task_epoch == epoch_number,
        info!("DRB computation is not for the next epoch.")
    );

    ensure!(
        computation.is_finished(),
        info!("DRB computation has not yet finished.")
    );

    match computation.await {
        Ok(result) => {
            let mut consensus_writer = task_state.consensus.write().await;
            consensus_writer
                .drb_seeds_and_results
                .results
                .insert(epoch_number, result);
            task_state.drb_computation = None;
            Ok(result)
        }
        Err(e) => Err(warn!("Error in DRB calculation: {:?}.", e)),
    }
}

/// Verify the DRB result from the proposal for the next epoch if this is the last block of the
/// current epoch.
///
/// Uses the result from `start_drb_task`.
///
/// Returns an error if we should not vote.
async fn verify_drb_result<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions>(
    proposal: &QuorumProposal2<TYPES>,
    task_state: &mut QuorumVoteTaskState<TYPES, I, V>,
) -> Result<()> {
    // Skip if this is not the expected block.
    if task_state.epoch_height == 0
        || !is_last_block_in_epoch(
            proposal.block_header.block_number(),
            task_state.epoch_height,
        )
    {
        tracing::debug!("Skipping DRB result verification");
        return Ok(());
    }

    let epoch = TYPES::Epoch::new(epoch_from_block_number(
        proposal.block_header.block_number(),
        task_state.epoch_height,
    ));

    let proposal_result = proposal
        .next_drb_result
        .context(info!("Proposal is missing the DRB result."))?;

    let membership_reader = task_state.membership.read().await;

    let has_stake_current_epoch = membership_reader.has_stake(&task_state.public_key, epoch);
    let has_stake_next_epoch = membership_reader.has_stake(&task_state.public_key, epoch + 1);

    drop(membership_reader);

    if has_stake_current_epoch {
        let computed_result = store_and_get_computed_drb_result(epoch + 1, task_state).await?;

        ensure!(proposal_result == computed_result, warn!("Our calculated DRB result is {:?}, which does not match the proposed DRB result of {:?}", computed_result, proposal_result));

        Ok(())
    } else if has_stake_next_epoch {
        store_received_drb_result(epoch + 1, proposal_result, task_state).await
    } else {
        Err(error!(
            "We are not participating in either the current or next epoch"
        ))
    }
}

/// Start the DRB computation task for the next epoch.
///
/// Uses the seed previously stored in `store_drb_seed_and_result`.
async fn start_drb_task<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions>(
    proposal: &QuorumProposal2<TYPES>,
    task_state: &mut QuorumVoteTaskState<TYPES, I, V>,
) {
    let current_epoch_number = TYPES::Epoch::new(epoch_from_block_number(
        proposal.block_header.block_number(),
        task_state.epoch_height,
    ));

    // Start the new task if we're in the committee for this epoch
    if task_state
        .membership
        .read()
        .await
        .has_stake(&task_state.public_key, current_epoch_number)
    {
        let new_epoch_number = current_epoch_number + 1;

        // If a task is currently live AND has finished, join it and save the result.
        // If the epoch for the calculation was the same as the provided epoch, return.
        // If a task is currently live and NOT finished, abort it UNLESS the task epoch is the
        // same as cur_epoch, in which case keep letting it run and return.
        // Continue the function if a task should be spawned for the given epoch.
        if let Some((task_epoch, join_handle)) = &mut task_state.drb_computation {
            if join_handle.is_finished() {
                match join_handle.await {
                    Ok(result) => {
                        task_state
                            .consensus
                            .write()
                            .await
                            .drb_seeds_and_results
                            .results
                            .insert(*task_epoch, result);
                        task_state.drb_computation = None;
                    }
                    Err(e) => {
                        tracing::error!("error joining DRB computation task: {e:?}");
                    }
                }
            } else if *task_epoch == new_epoch_number {
                return;
            } else {
                join_handle.abort();
                task_state.drb_computation = None;
            }
        }

        // In case we somehow ended up processing this epoch already, don't start it again
        let mut consensus_writer = task_state.consensus.write().await;
        if consensus_writer
            .drb_seeds_and_results
            .results
            .contains_key(&new_epoch_number)
        {
            return;
        }

        if let Entry::Occupied(entry) = consensus_writer
            .drb_seeds_and_results
            .seeds
            .entry(new_epoch_number)
        {
            let drb_seed_input = *entry.get();
            let new_drb_task = spawn(async move { compute_drb_result::<TYPES>(drb_seed_input) });
            task_state.drb_computation = Some((new_epoch_number, new_drb_task));
            entry.remove();
        }
    }
}

/// Store the DRB seed two epochs in advance and the computed DRB result for next epoch.
///
/// We store the DRB seed and result if the decided block is the third from the last block in the
/// current epoch and for the former, if we are in the quorum committee of the next epoch.
///
/// Special cases:
/// * Epoch 0: No DRB computation since we'll transition to epoch 1 immediately.
/// * Epoch 1 and 2: No computed DRB result since when we first start the computation in epoch 1,
///   the result is for epoch 3.
///
/// We don't need to handle the special cases explicitly here, because the first proposal with
/// which we'll start the DRB computation is for epoch 3.
async fn store_drb_seed_and_result<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions>(
    proposal: &QuorumProposal2<TYPES>,
    task_state: &mut QuorumVoteTaskState<TYPES, I, V>,
    leaf_views: &[LeafInfo<TYPES>],
) -> Result<()> {
    // This is never none if we've reached a new decide, so this is safe to unwrap.
    let decided_block_number = leaf_views
        .last()
        .unwrap()
        .leaf
        .block_header()
        .block_number();

    // Skip if this is not the expected block.
    if task_state.epoch_height != 0 && is_epoch_root(decided_block_number, task_state.epoch_height)
    {
        // Cancel old DRB computation tasks.
        let current_epoch_number = TYPES::Epoch::new(epoch_from_block_number(
            decided_block_number,
            task_state.epoch_height,
        ));

        let mut consensus_writer = task_state.consensus.write().await;
        consensus_writer
            .drb_seeds_and_results
            .garbage_collect(current_epoch_number);
        drop(consensus_writer);

        // Store the DRB result for the next epoch, which will be used by the proposal task to
        // include in the proposal in the last block of this epoch.
        store_and_get_computed_drb_result(current_epoch_number + 1, task_state).await?;

        // Skip if we are not in the committee of the next epoch.
        if task_state
            .membership
            .read()
            .await
            .has_stake(&task_state.public_key, current_epoch_number + 1)
        {
            let new_epoch_number = current_epoch_number + 2;
            let Ok(drb_seed_input_vec) = bincode::serialize(&proposal.justify_qc.signatures) else {
                bail!("Failed to serialize the QC signature.");
            };
            let Ok(drb_seed_input) = drb_seed_input_vec.try_into() else {
                bail!("Failed to convert the serialized QC signature into a DRB seed input.");
            };

            // Store the DRB seed input for the epoch after the next one.
            task_state
                .consensus
                .write()
                .await
                .drb_seeds_and_results
                .store_seed(new_epoch_number, drb_seed_input);
        }
    }
    Ok(())
}

/// Handles the `QuorumProposalValidated` event.
#[instrument(skip_all, fields(id = task_state.id, view = *proposal.view_number))]
pub(crate) async fn handle_quorum_proposal_validated<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    V: Versions,
>(
    proposal: &QuorumProposal2<TYPES>,
    task_state: &mut QuorumVoteTaskState<TYPES, I, V>,
) -> Result<()> {
    let version = task_state
        .upgrade_lock
        .version(proposal.view_number())
        .await?;

    if version >= V::Epochs::VERSION {
        // Don't vote if the DRB result verification fails.
        verify_drb_result(proposal, task_state).await?;
        start_drb_task(proposal, task_state).await;
    }

    let LeafChainTraversalOutcome {
        new_locked_view_number,
        new_decided_view_number,
        new_decide_qc,
        leaf_views,
        included_txns,
        decided_upgrade_cert,
    } = if version >= V::Epochs::VERSION {
        decide_from_proposal_2(
            proposal,
            OuterConsensus::new(Arc::clone(&task_state.consensus.inner_consensus)),
            Arc::clone(&task_state.upgrade_lock.decided_upgrade_certificate),
            &task_state.public_key,
            version >= V::Epochs::VERSION,
            &task_state.membership,
        )
        .await
    } else {
        decide_from_proposal(
            proposal,
            OuterConsensus::new(Arc::clone(&task_state.consensus.inner_consensus)),
            Arc::clone(&task_state.upgrade_lock.decided_upgrade_certificate),
            &task_state.public_key,
            version >= V::Epochs::VERSION,
            &task_state.membership,
        )
        .await
    };

    if let Some(cert) = decided_upgrade_cert.clone() {
        let mut decided_certificate_lock = task_state
            .upgrade_lock
            .decided_upgrade_certificate
            .write()
            .await;
        *decided_certificate_lock = Some(cert.clone());
        drop(decided_certificate_lock);

        let _ = task_state
            .storage
            .write()
            .await
            .update_decided_upgrade_certificate(Some(cert.clone()))
            .await;
    }

    let mut consensus_writer = task_state.consensus.write().await;
    if let Some(locked_view_number) = new_locked_view_number {
        consensus_writer.update_locked_view(locked_view_number)?;
    }

    #[allow(clippy::cast_precision_loss)]
    if let Some(decided_view_number) = new_decided_view_number {
        // Bring in the cleanup crew. When a new decide is indeed valid, we need to clear out old memory.

        let old_decided_view = consensus_writer.last_decided_view();
        consensus_writer.collect_garbage(old_decided_view, decided_view_number);

        // Set the new decided view.
        consensus_writer.update_last_decided_view(decided_view_number)?;

        consensus_writer
            .metrics
            .last_decided_time
            .set(Utc::now().timestamp().try_into().unwrap());
        consensus_writer.metrics.invalid_qc.set(0);
        consensus_writer
            .metrics
            .last_decided_view
            .set(usize::try_from(consensus_writer.last_decided_view().u64()).unwrap());
        let cur_number_of_views_per_decide_event =
            *proposal.view_number() - consensus_writer.last_decided_view().u64();
        consensus_writer
            .metrics
            .number_of_views_per_decide_event
            .add_point(cur_number_of_views_per_decide_event as f64);

        tracing::debug!(
            "Sending Decide for view {:?}",
            consensus_writer.last_decided_view()
        );

        // We don't need to hold this while we broadcast
        drop(consensus_writer);

        // Send an update to everyone saying that we've reached a decide
        broadcast_event(
            Event {
                view_number: decided_view_number,
                event: EventType::Decide {
                    leaf_chain: Arc::new(leaf_views.clone()),
                    // This is never none if we've reached a new decide, so this is safe to unwrap.
                    qc: Arc::new(new_decide_qc.unwrap()),
                    block_size: included_txns.map(|txns| txns.len().try_into().unwrap()),
                },
            },
            &task_state.output_event_stream,
        )
        .await;
        tracing::debug!("Successfully sent decide event");

        if version >= V::Epochs::VERSION {
            store_drb_seed_and_result(proposal, task_state, &leaf_views).await?;
        }
    }

    Ok(())
}

/// Updates the shared consensus state with the new voting data.
#[instrument(skip_all, target = "VoteDependencyHandle", fields(view = *view_number))]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn update_shared_state<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    V: Versions,
>(
    consensus: OuterConsensus<TYPES>,
    sender: Sender<Arc<HotShotEvent<TYPES>>>,
    receiver: InactiveReceiver<Arc<HotShotEvent<TYPES>>>,
    membership: Arc<RwLock<TYPES::Membership>>,
    public_key: TYPES::SignatureKey,
    private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
    upgrade_lock: UpgradeLock<TYPES, V>,
    view_number: TYPES::View,
    instance_state: Arc<TYPES::InstanceState>,
    storage: Arc<RwLock<I::Storage>>,
    proposed_leaf: &Leaf2<TYPES>,
    vid_share: &Proposal<TYPES, VidDisperseShare2<TYPES>>,
    parent_view_number: Option<TYPES::View>,
    epoch_height: u64,
) -> Result<()> {
    let justify_qc = &proposed_leaf.justify_qc();

    let consensus_reader = consensus.read().await;
    // Try to find the validated view within the validated state map. This will be present
    // if we have the saved leaf, but if not we'll get it when we fetch_proposal.
    let mut maybe_validated_view = parent_view_number.and_then(|view_number| {
        consensus_reader
            .validated_state_map()
            .get(&view_number)
            .cloned()
    });

    // Justify qc's leaf commitment should be the same as the parent's leaf commitment.
    let mut maybe_parent = consensus_reader
        .saved_leaves()
        .get(&justify_qc.data.leaf_commit)
        .cloned();

    drop(consensus_reader);

    maybe_parent = match maybe_parent {
        Some(p) => Some(p),
        None => {
            match fetch_proposal(
                justify_qc.view_number(),
                sender.clone(),
                receiver.activate_cloned(),
                Arc::clone(&membership),
                OuterConsensus::new(Arc::clone(&consensus.inner_consensus)),
                public_key.clone(),
                private_key.clone(),
                &upgrade_lock,
                epoch_height,
            )
            .await
            .ok()
            {
                Some((leaf, view)) => {
                    maybe_validated_view = Some(view);
                    Some(leaf)
                }
                None => None,
            }
        }
    };

    let parent = maybe_parent.context(info!(
        "Proposal's parent missing from storage with commitment: {:?}, proposal view {:?}",
        justify_qc.data.leaf_commit,
        proposed_leaf.view_number(),
    ))?;

    let Some(validated_view) = maybe_validated_view else {
        bail!(
            "Failed to fetch view for parent, parent view {:?}",
            parent_view_number
        );
    };

    let (Some(parent_state), _) = validated_view.state_and_delta() else {
        bail!("Parent state not found! Consensus internally inconsistent");
    };

    let version = upgrade_lock.version(view_number).await?;

    let (validated_state, state_delta) = parent_state
        .validate_and_apply_header(
            &instance_state,
            &parent,
            &proposed_leaf.block_header().clone(),
            vid_share.data.common.clone(),
            version,
            *view_number,
        )
        .await
        .wrap()
        .context(warn!("Block header doesn't extend the proposal!"))?;

    let state = Arc::new(validated_state);
    let delta = Arc::new(state_delta);

    // Now that we've rounded everyone up, we need to update the shared state
    let mut consensus_writer = consensus.write().await;

    if let Err(e) = consensus_writer.update_leaf(
        proposed_leaf.clone(),
        Arc::clone(&state),
        Some(Arc::clone(&delta)),
    ) {
        tracing::trace!("{e:?}");
    }

    // Kick back our updated structures for downstream usage.
    let new_leaves = consensus_writer.saved_leaves().clone();
    let new_state = consensus_writer.validated_state_map().clone();
    drop(consensus_writer);

    // Send the new state up to the sequencer.
    storage
        .write()
        .await
        .update_undecided_state2(new_leaves, new_state)
        .await
        .wrap()
        .context(error!("Failed to update undecided state"))?;

    Ok(())
}

/// Submits the `QuorumVoteSend` event if all the dependencies are met.
#[instrument(skip_all, fields(name = "Submit quorum vote", level = "error"))]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn submit_vote<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions>(
    sender: Sender<Arc<HotShotEvent<TYPES>>>,
    membership: Arc<RwLock<TYPES::Membership>>,
    public_key: TYPES::SignatureKey,
    private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
    upgrade_lock: UpgradeLock<TYPES, V>,
    view_number: TYPES::View,
    storage: Arc<RwLock<I::Storage>>,
    leaf: Leaf2<TYPES>,
    vid_share: Proposal<TYPES, VidDisperseShare2<TYPES>>,
    extended_vote: bool,
    epoch_height: u64,
) -> Result<()> {
    let epoch_number = TYPES::Epoch::new(epoch_from_block_number(
        leaf.block_header().block_number(),
        epoch_height,
    ));

    let membership_reader = membership.read().await;
    let committee_member_in_current_epoch = membership_reader.has_stake(&public_key, epoch_number);
    // If the proposed leaf is for the last block in the epoch and the node is part of the quorum committee
    // in the next epoch, the node should vote to achieve the double quorum.
    let committee_member_in_next_epoch = is_last_block_in_epoch(leaf.height(), epoch_height)
        && membership_reader.has_stake(&public_key, epoch_number + 1);
    drop(membership_reader);

    ensure!(
        committee_member_in_current_epoch || committee_member_in_next_epoch,
        info!(
            "We were not chosen for quorum committee on {:?}",
            view_number
        )
    );

    // Create and send the vote.
    let vote = QuorumVote2::<TYPES>::create_signed_vote(
        QuorumData2 {
            leaf_commit: leaf.commit(),
            epoch: epoch_number,
        },
        view_number,
        &public_key,
        &private_key,
        &upgrade_lock,
    )
    .await
    .wrap()
    .context(error!("Failed to sign vote. This should never happen."))?;
    // Add to the storage.
    storage
        .write()
        .await
        .append_vid2(&vid_share)
        .await
        .wrap()
        .context(error!("Failed to store VID share"))?;

    if extended_vote {
        tracing::debug!("sending extended vote to everybody",);
        broadcast_event(
            Arc::new(HotShotEvent::ExtendedQuorumVoteSend(vote)),
            &sender,
        )
        .await;
    } else {
        tracing::debug!(
            "sending vote to next quorum leader {:?}",
            vote.view_number() + 1
        );
        broadcast_event(Arc::new(HotShotEvent::QuorumVoteSend(vote)), &sender).await;
    }

    Ok(())
}
