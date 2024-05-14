// TODO - delete this after dependency tasks merges.
#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use async_lock::RwLockUpgradableReadGuard;
use std::sync::Arc;
use tracing::debug;

use async_broadcast::{broadcast, Sender};
use hotshot_types::{
    data::QuorumProposal,
    message::Proposal,
    traits::{
        election::Membership,
        node_implementation::{NodeImplementation, NodeType},
        storage::Storage,
    },
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

    let view = proposal.data.get_view_number();
    let view_leader_key = task_state.quorum_membership.get_leader(view);
    let justify_qc = proposal.data.justify_qc.clone();

    if !justify_qc.is_valid_cert(task_state.quorum_membership.as_ref()) {
        let consensus = task_state.consensus.read().await;
        consensus.metrics.invalid_qc.update(1);
        bail!("Invalid justify_qc in proposal for view {}", *view);
    }

    // NOTE: We could update our view with a valid TC but invalid QC, but that is not what we do here
    if let Err(e) = update_view::<TYPES>(
        view,
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
        .get(&justify_qc.get_data().leaf_commit)
        .cloned()
    {
        Some(leaf) => {
            if let (Some(state), _) = consensus_read.get_state_and_delta(leaf.get_view_number()) {
                Some((leaf, Arc::clone(&state)))
            } else {
                bail!("Parent state not found! Consensus internally inconsistent");
            }
        }
        None => None,
    };

    if justify_qc.get_view_number() > consensus_read.high_qc().view_number {
        if let Err(e) = task_state
            .storage
            .write()
            .await
            .update_high_qc(justify_qc.clone())
            .await
        {
            bail!("Failed to store High QC not voting. Error: {:?}", e);
        }
    }

    let mut consensus_write = RwLockUpgradableReadGuard::upgrade(consensus_read).await;
    if let Err(e) = consensus_write.update_high_qc(justify_qc.clone()) {
        tracing::trace!("{e:?}");
    }

    let Some((parent_leaf, _parent_state)) = parent else {
        // TODO - liveness check.
        todo!()
    };

    // Drop the lock and broadcast the high qc update.
    drop(consensus_write);

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
