// TODO - delete this after dependency tasks merges.
#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use async_lock::RwLockUpgradableReadGuard;
use committable::Committable;
use std::sync::Arc;
use tracing::{debug, warn};

use async_broadcast::{broadcast, Sender};
use hotshot_types::{
    consensus::ProposalDependencyData,
    data::{Leaf, QuorumProposal},
    message::Proposal,
    simple_certificate::QuorumCertificate,
    traits::{
        election::Membership,
        node_implementation::{NodeImplementation, NodeType},
        storage::Storage,
        ValidatedState,
    },
    utils::{View, ViewInner},
    vote::{Certificate, HasViewNumber},
};

use crate::{
    consensus::{
        helpers::{validate_proposal_safety_and_liveness, validate_proposal_view_and_certs},
        view_change::{update_view, SEND_VIEW_CHANGE_EVENT},
    },
    events::HotShotEvent,
    helpers::broadcast_event,
};

use super::QuorumProposalRecvTaskState;

/// Broadcast the liveness proposal in the event that the parent state is not found for
/// a given `proposal`.
async fn send_liveness_proposal<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    proposal: &Proposal<TYPES, QuorumProposal<TYPES>>,
    event_sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    justify_qc: &QuorumCertificate<TYPES>,
    task_state: &mut QuorumProposalRecvTaskState<TYPES, I>,
) -> Option<QuorumProposal<TYPES>> {
    let view_number = proposal.data.view_number();
    let mut consensus_write = task_state.consensus.write().await;

    warn!(
        "Proposal's parent missing from storage with commitment: {:?}",
        justify_qc.data.leaf_commit
    );
    let leaf = Leaf::from_quorum_proposal(&proposal.data);

    let state = Arc::new(
        <TYPES::ValidatedState as ValidatedState<TYPES>>::from_header(&proposal.data.block_header),
    );
    let view = View {
        view_inner: ViewInner::Leaf {
            leaf: leaf.commit(),
            state,
            delta: None,
        },
    };

    consensus_write.update_validated_state_map(view_number, view.clone());
    broadcast_event(
        HotShotEvent::ValidatedStateUpdated(view_number, view.clone()).into(),
        event_sender,
    )
    .await;

    consensus_write.update_saved_leaves(leaf.clone());
    broadcast_event(
        HotShotEvent::NewUndecidedView(leaf.clone()).into(),
        event_sender,
    )
    .await;

    if let Err(e) = task_state
        .storage
        .write()
        .await
        .update_undecided_state(
            consensus_write.saved_leaves().clone(),
            consensus_write.validated_state_map().clone(),
        )
        .await
    {
        warn!("Couldn't store undecided state.  Error: {:?}", e);
    }

    let liveness_check = justify_qc.view_number() > consensus_write.locked_view();

    let high_qc = consensus_write.high_qc().clone();
    drop(consensus_write);

    // Broadcast that we've updated our consensus state so that other tasks know it's safe to grab.
    broadcast_event(
        HotShotEvent::ValidatedStateUpdated(view_number, view).into(),
        event_sender,
    )
    .await;
    broadcast_event(
        HotShotEvent::NewUndecidedView(leaf.clone()).into(),
        event_sender,
    )
    .await;

    let mut current_proposal = None;
    if liveness_check {
        current_proposal = Some(proposal.data.clone());

        let new_view = proposal.data.view_number + 1;

        // This is for the case where we form a QC but have not yet seen the previous proposal ourselves
        let should_propose = task_state.quorum_membership.leader(new_view) == task_state.public_key
            && high_qc.view_number == current_proposal.clone().unwrap().view_number;

        if should_propose {
            debug!(
                "Attempting to publish proposal after voting for liveness; now in view: {}",
                *new_view
            );
            // TODO - publish the propose now event.
        }
    }

    // If we are missing the parent from storage, the safety check will fail.  But we can
    // still vote if the liveness check succeeds.
    current_proposal
}

/// Handles the `QuorumProposalRecv` event.
pub(crate) async fn handle_quorum_proposal_recv<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    proposal: &Proposal<TYPES, QuorumProposal<TYPES>>,
    sender: &TYPES::SignatureKey,
    event_sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    task_state: &mut QuorumProposalRecvTaskState<TYPES, I>,
) -> Result<Option<QuorumProposal<TYPES>>> {
    let sender = sender.clone();

    validate_proposal_view_and_certs(
        proposal,
        &sender,
        task_state.cur_view,
        &task_state.quorum_membership,
        &task_state.timeout_membership,
    )
    .context("Failed to validate proposal view and attached certs")?;

    let view_number = proposal.data.view_number();
    let view_leader_key = task_state.quorum_membership.leader(view_number);
    let justify_qc = proposal.data.justify_qc.clone();

    if !justify_qc.is_valid_cert(task_state.quorum_membership.as_ref()) {
        let consensus = task_state.consensus.read().await;
        consensus.metrics.invalid_qc.update(1);
        bail!("Invalid justify_qc in proposal for view {}", *view_number);
    }

    // NOTE: We could update our view with a valid TC but invalid QC, but that is not what we do here
    if let Err(e) = update_view::<TYPES>(
        view_number,
        event_sender,
        task_state.timeout,
        Arc::clone(&task_state.consensus),
        &mut task_state.cur_view,
        &mut task_state.timeout_task,
        SEND_VIEW_CHANGE_EVENT,
    )
    .await
    {
        debug!("Failed to update view; error = {e:#}");
    }

    let consensus_read = task_state.consensus.upgradable_read().await;

    // Get the parent leaf and state.
    let parent = match consensus_read
        .saved_leaves()
        .get(&justify_qc.data.leaf_commit)
        .cloned()
    {
        Some(leaf) => {
            if let (Some(state), _) = consensus_read.state_and_delta(leaf.view_number()) {
                Some((leaf, Arc::clone(&state)))
            } else {
                tracing::error!(?leaf, "FUCK");
                bail!("Parent state not found! Consensus internally inconsistent");
            }
        }
        None => None,
    };

    if justify_qc.view_number() > consensus_read.high_qc().view_number {
        if let Err(e) = task_state
            .storage
            .write()
            .await
            .update_high_qc(justify_qc.clone())
            .await
        {
            bail!("Failed to store High QC, not voting; error = {:?}", e);
        }
    }

    let mut consensus_write = RwLockUpgradableReadGuard::upgrade(consensus_read).await;
    if let Err(e) = consensus_write.update_high_qc(justify_qc.clone()) {
        tracing::trace!("{e:?}");
    }

    // Drop the lock before stepping into the liveness proposal check, we'll let it
    // decide if it needs to send it.
    drop(consensus_write);

    let Some((parent_leaf, _parent_state)) = parent else {
        return Ok(send_liveness_proposal(proposal, event_sender, &justify_qc, task_state).await);
    };

    broadcast_event(
        HotShotEvent::HighQcUpdated(justify_qc.clone()).into(),
        event_sender,
    )
    .await;

    // Validate the proposal
    validate_proposal_safety_and_liveness(
        proposal.clone(),
        parent_leaf,
        Arc::clone(&task_state.consensus),
        None,
        Arc::clone(&task_state.quorum_membership),
        view_leader_key,
        event_sender.clone(),
        sender,
        task_state.output_event_stream.clone(),
    )
    .await?;

    Ok(None)
}
