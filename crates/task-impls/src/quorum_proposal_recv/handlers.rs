// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

#![allow(dead_code)]

use std::sync::Arc;

use async_broadcast::{broadcast, Receiver, Sender};
use async_lock::RwLockUpgradableReadGuard;
use committable::Committable;
use hotshot_types::{
    consensus::OuterConsensus,
    data::{Leaf2, QuorumProposal, QuorumProposal2},
    message::Proposal,
    simple_certificate::QuorumCertificate,
    traits::{
        block_contents::BlockHeader,
        election::Membership,
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
        signature_key::SignatureKey,
        storage::Storage,
        ValidatedState,
    },
    utils::{epoch_from_block_number, View, ViewInner},
    vote::{Certificate, HasViewNumber},
};
use tokio::spawn;
use tracing::instrument;
use utils::anytrace::*;
use vbs::version::StaticVersionType;

use super::{QuorumProposalRecvTaskState, ValidationInfo};
use crate::{
    events::HotShotEvent,
    helpers::{
        broadcast_event, fetch_proposal, validate_proposal_safety_and_liveness,
        validate_proposal_view_and_certs,
    },
    quorum_proposal_recv::{UpgradeLock, Versions},
};
/// Update states in the event that the parent state is not found for a given `proposal`.
#[instrument(skip_all)]
async fn validate_proposal_liveness<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions>(
    proposal: &Proposal<TYPES, QuorumProposal2<TYPES>>,
    validation_info: &ValidationInfo<TYPES, I, V>,
) -> Result<()> {
    let mut consensus_writer = validation_info.consensus.write().await;

    let leaf = Leaf2::from_quorum_proposal(&proposal.data);

    let state = Arc::new(
        <TYPES::ValidatedState as ValidatedState<TYPES>>::from_header(&proposal.data.block_header),
    );

    if let Err(e) = consensus_writer.update_leaf(leaf.clone(), state, None) {
        tracing::trace!("{e:?}");
    }

    if let Err(e) = validation_info
        .storage
        .write()
        .await
        .update_undecided_state2(
            consensus_writer.saved_leaves().clone(),
            consensus_writer.validated_state_map().clone(),
        )
        .await
    {
        tracing::warn!("Couldn't store undecided state.  Error: {:?}", e);
    }

    let liveness_check =
        proposal.data.justify_qc.clone().view_number() > consensus_writer.locked_view();
    // if we are using HS2 we update our locked view for any QC from a leader greater than our current lock
    if liveness_check
        && validation_info
            .upgrade_lock
            .version(leaf.view_number())
            .await
            .is_ok_and(|v| v >= V::Epochs::VERSION)
    {
        consensus_writer.update_locked_view(proposal.data.justify_qc.clone().view_number())?;
    }

    drop(consensus_writer);

    if !liveness_check {
        bail!("Quorum Proposal failed the liveness check");
    }

    Ok(())
}

/// Spawn a task which will fire a request to get a proposal, and store it.
#[allow(clippy::too_many_arguments)]
fn spawn_fetch_proposal<TYPES: NodeType, V: Versions>(
    view: TYPES::View,
    event_sender: Sender<Arc<HotShotEvent<TYPES>>>,
    event_receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
    membership: Arc<TYPES::Membership>,
    consensus: OuterConsensus<TYPES>,
    sender_public_key: TYPES::SignatureKey,
    sender_private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
    upgrade_lock: UpgradeLock<TYPES, V>,
    epoch_height: u64,
) {
    spawn(async move {
        let lock = upgrade_lock;

        let _ = fetch_proposal(
            view,
            event_sender,
            event_receiver,
            membership,
            consensus,
            sender_public_key,
            sender_private_key,
            &lock,
            epoch_height,
        )
        .await;
    });
}

/// Handles the `QuorumProposalRecv` event by first validating the cert itself for the view, and then
/// updating the states, which runs when the proposal cannot be found in the internal state map.
///
/// This code can fail when:
/// - The justify qc is invalid.
/// - The task is internally inconsistent.
/// - The sequencer storage update fails.
#[allow(clippy::too_many_lines)]
#[instrument(skip_all)]
pub(crate) async fn handle_quorum_proposal_recv<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    V: Versions,
>(
    proposal: &Proposal<TYPES, QuorumProposal2<TYPES>>,
    quorum_proposal_sender_key: &TYPES::SignatureKey,
    event_sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    event_receiver: &Receiver<Arc<HotShotEvent<TYPES>>>,
    validation_info: ValidationInfo<TYPES, I, V>,
) -> Result<()> {
    let quorum_proposal_sender_key = quorum_proposal_sender_key.clone();

    validate_proposal_view_and_certs(proposal, &validation_info)
        .await
        .context(warn!("Failed to validate proposal view or attached certs"))?;

    let view_number = proposal.data.view_number();
    let justify_qc = proposal.data.justify_qc.clone();
    let proposal_block_number = proposal.data.block_header.block_number();
    let proposal_epoch = TYPES::Epoch::new(epoch_from_block_number(
        proposal_block_number,
        validation_info.epoch_height,
    ));

    if !justify_qc
        .is_valid_cert(
            validation_info
                .quorum_membership
                .stake_table(justify_qc.data.epoch),
            validation_info
                .quorum_membership
                .success_threshold(justify_qc.data.epoch),
            &validation_info.upgrade_lock,
        )
        .await
    {
        let consensus_reader = validation_info.consensus.read().await;
        consensus_reader.metrics.invalid_qc.update(1);
        bail!("Invalid justify_qc in proposal for view {}", *view_number);
    }

    broadcast_event(
        Arc::new(HotShotEvent::QuorumProposalPreliminarilyValidated(
            proposal.clone(),
        )),
        event_sender,
    )
    .await;

    // Get the parent leaf and state.
    let parent_leaf = validation_info
        .consensus
        .read()
        .await
        .saved_leaves()
        .get(&justify_qc.data.leaf_commit)
        .cloned();

    if parent_leaf.is_none() {
        spawn_fetch_proposal(
            justify_qc.view_number(),
            event_sender.clone(),
            event_receiver.clone(),
            Arc::clone(&validation_info.quorum_membership),
            OuterConsensus::new(Arc::clone(&validation_info.consensus.inner_consensus)),
            // Note that we explicitly use the node key here instead of the provided key in the signature.
            // This is because the key that we receive is for the prior leader, so the payload would be routed
            // incorrectly.
            validation_info.public_key.clone(),
            validation_info.private_key.clone(),
            validation_info.upgrade_lock.clone(),
            validation_info.epoch_height,
        );
    }
    let consensus_reader = validation_info.consensus.read().await;

    let parent = match parent_leaf {
        Some(leaf) => {
            if let (Some(state), _) = consensus_reader.state_and_delta(leaf.view_number()) {
                Some((leaf, Arc::clone(&state)))
            } else {
                bail!("Parent state not found! Consensus internally inconsistent");
            }
        }
        None => None,
    };

    if justify_qc.view_number() > consensus_reader.high_qc().view_number {
        if let Err(e) = validation_info
            .storage
            .write()
            .await
            .update_high_qc2(justify_qc.clone())
            .await
        {
            bail!("Failed to store High QC, not voting; error = {:?}", e);
        }
    }
    drop(consensus_reader);

    let mut consensus_writer = validation_info.consensus.write().await;
    if let Err(e) = consensus_writer.update_high_qc(justify_qc.clone()) {
        tracing::trace!("{e:?}");
    }
    drop(consensus_writer);

    let Some((parent_leaf, _parent_state)) = parent else {
        tracing::warn!(
            "Proposal's parent missing from storage with commitment: {:?}",
            justify_qc.data.leaf_commit
        );
        validate_proposal_liveness(proposal, &validation_info).await?;
        tracing::trace!(
            "Sending ViewChange for view {} and epoch {}",
            view_number,
            *proposal_epoch
        );
        broadcast_event(
            Arc::new(HotShotEvent::ViewChange(view_number, proposal_epoch)),
            event_sender,
        )
        .await;
        return Ok(());
    };

    // Validate the proposal
    validate_proposal_safety_and_liveness::<TYPES, I, V>(
        proposal.clone(),
        parent_leaf,
        &validation_info,
        event_sender.clone(),
        quorum_proposal_sender_key,
    )
    .await?;

    tracing::trace!(
        "Sending ViewChange for view {} and epoch {}",
        view_number,
        *proposal_epoch
    );
    broadcast_event(
        Arc::new(HotShotEvent::ViewChange(view_number, proposal_epoch)),
        event_sender,
    )
    .await;

    Ok(())
}
