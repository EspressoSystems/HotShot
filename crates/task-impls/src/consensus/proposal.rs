use std::sync::Arc;

use crate::{events::HotShotEvent, helpers::broadcast_event};
use anyhow::{bail, ensure, Context, Result};
use async_broadcast::Sender;
use async_compatibility_layer::art::async_sleep;
use async_lock::{RwLock, RwLockUpgradableReadGuard};
use committable::Committable;
use core::time::Duration;
use hotshot_types::{
    consensus::{CommitmentAndMetadata, Consensus, View},
    data::{Leaf, QuorumProposal, ViewChangeEvidence},
    event::{Event, EventType},
    message::Proposal,
    simple_certificate::{QuorumCertificate, UpgradeCertificate},
    traits::{
        block_contents::BlockHeader, election::Membership, node_implementation::NodeType,
        signature_key::SignatureKey, states::ValidatedState, storage::Storage,
    },
    utils::{Terminator, ViewInner},
    vote::{Certificate, HasViewNumber},
};
use tracing::{debug, error, warn};

#[cfg(not(feature = "dependency-tasks"))]
use std::marker::PhantomData;

/// Validate the state and safety and liveness of a proposal then emit
/// a `QuorumProposalValidated` event.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)]
pub async fn validate_proposal_safety_and_liveness<TYPES: NodeType>(
    proposal: Proposal<TYPES, QuorumProposal<TYPES>>,
    parent_leaf: Leaf<TYPES>,
    consensus: Arc<RwLock<Consensus<TYPES>>>,
    decided_upgrade_certificate: Option<UpgradeCertificate<TYPES>>,
    quorum_membership: Arc<TYPES::Membership>,
    parent_state: Arc<TYPES::ValidatedState>,
    view_leader_key: TYPES::SignatureKey,
    event_stream: Sender<Arc<HotShotEvent<TYPES>>>,
    sender: TYPES::SignatureKey,
    event_sender: Sender<Event<TYPES>>,
    storage: Arc<RwLock<impl Storage<TYPES>>>,
) -> Result<()> {
    let (validated_state, state_delta) = parent_state
        .validate_and_apply_header(
            &consensus.read().await.instance_state,
            &parent_leaf,
            &proposal.data.block_header.clone(),
        )
        .await
        .context("Block header doesn't extend the proposal!")?;

    let state = Arc::new(validated_state);
    let delta = Arc::new(state_delta);
    let parent_commitment = parent_leaf.commit();
    let view = proposal.data.get_view_number();

    let mut proposed_leaf = Leaf::from_quorum_proposal(&proposal.data);
    proposed_leaf.set_parent_commitment(parent_commitment);

    // Validate the proposal's signature. This should also catch if the leaf_commitment does not equal our calculated parent commitment
    //
    // There is a mistake here originating in the genesis leaf/qc commit. This should be replaced by:
    //
    //    proposal.validate_signature(&quorum_membership)?;
    //
    // in a future PR.
    ensure!(
        view_leader_key.validate(&proposal.signature, proposed_leaf.commit().as_ref()),
        "Could not verify proposal."
    );

    UpgradeCertificate::validate(&proposal.data.upgrade_certificate, &quorum_membership)?;

    // Validate that the upgrade certificate is re-attached, if we saw one on the parent
    proposed_leaf.extends_upgrade(&parent_leaf, &decided_upgrade_certificate)?;

    let justify_qc = proposal.data.justify_qc.clone();
    // Create a positive vote if either liveness or safety check
    // passes.

    // Liveness check.
    let consensus = consensus.upgradable_read().await;
    let liveness_check = justify_qc.get_view_number() > consensus.locked_view;

    // Safety check.
    // Check if proposal extends from the locked leaf.
    let outcome = consensus.visit_leaf_ancestors(
        justify_qc.get_view_number(),
        Terminator::Inclusive(consensus.locked_view),
        false,
        |leaf, _, _| {
            // if leaf view no == locked view no then we're done, report success by
            // returning true
            leaf.get_view_number() != consensus.locked_view
        },
    );
    let safety_check = outcome.is_ok();

    ensure!(safety_check || liveness_check, {
        if let Err(e) = outcome {
            broadcast_event(
                Event {
                    view_number: view,
                    event: EventType::Error { error: Arc::new(e) },
                },
                &event_sender,
            )
            .await;
        }

        format!("Failed safety and liveness check \n High QC is {:?}  Proposal QC is {:?}  Locked view is {:?}", consensus.high_qc, proposal.data.clone(), consensus.locked_view)
    });

    // We accept the proposal, notify the application layer

    broadcast_event(
        Event {
            view_number: view,
            event: EventType::QuorumProposal {
                proposal: proposal.clone(),
                sender,
            },
        },
        &event_sender,
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

    let mut consensus = RwLockUpgradableReadGuard::upgrade(consensus).await;

    consensus.validated_state_map.insert(
        view,
        View {
            view_inner: ViewInner::Leaf {
                leaf: proposed_leaf.commit(),
                state: state.clone(),
                delta: Some(delta.clone()),
            },
        },
    );
    consensus
        .saved_leaves
        .insert(proposed_leaf.commit(), proposed_leaf.clone());

    if let Err(e) = storage
        .write()
        .await
        .update_undecided_state(
            consensus.saved_leaves.clone(),
            consensus.validated_state_map.clone(),
        )
        .await
    {
        warn!("Couldn't store undecided state.  Error: {:?}", e);
    }

    Ok(())
}

/// Create the header for a proposal, build the proposal, and broadcast
/// the proposal send evnet.
#[allow(clippy::too_many_arguments)]
#[cfg(not(feature = "dependency-tasks"))]
pub async fn create_and_send_proposal<TYPES: NodeType>(
    pub_key: TYPES::SignatureKey,
    private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
    consensus: Arc<RwLock<Consensus<TYPES>>>,
    event_stream: Sender<Arc<HotShotEvent<TYPES>>>,
    view: TYPES::Time,
    commitment_and_metadata: CommitmentAndMetadata<TYPES>,
    parent_leaf: Leaf<TYPES>,
    state: Arc<TYPES::ValidatedState>,
    upgrade_cert: Option<UpgradeCertificate<TYPES>>,
    proposal_cert: Option<ViewChangeEvidence<TYPES>>,
    round_start_delay: u64,
) {
    let block_header = TYPES::BlockHeader::new(
        state.as_ref(),
        &consensus.read().await.instance_state,
        &parent_leaf,
        commitment_and_metadata.commitment,
        commitment_and_metadata.builder_commitment,
        commitment_and_metadata.metadata,
        commitment_and_metadata.fee,
    )
    .await;

    let proposal = QuorumProposal {
        block_header,
        view_number: view,
        justify_qc: consensus.read().await.high_qc.clone(),
        proposal_certificate: proposal_cert,
        upgrade_certificate: upgrade_cert,
    };

    let mut proposed_leaf = Leaf::from_quorum_proposal(&proposal);
    proposed_leaf.set_parent_commitment(parent_leaf.commit());

    let Ok(signature) = TYPES::SignatureKey::sign(&private_key, proposed_leaf.commit().as_ref())
    else {
        // This should never happen.
        error!("Failed to sign proposed_leaf.commit()!");
        return;
    };

    let message = Proposal {
        data: proposal,
        signature,
        _pd: PhantomData,
    };
    debug!(
        "Sending null proposal for view {:?} \n {:?}",
        proposed_leaf.get_view_number(),
        ""
    );

    async_sleep(Duration::from_millis(round_start_delay)).await;
    broadcast_event(
        Arc::new(HotShotEvent::QuorumProposalSend(message.clone(), pub_key)),
        &event_stream,
    )
    .await;
}

pub fn validate_proposal_view_and_certs<TYPES: NodeType>(
    proposal: &Proposal<TYPES, QuorumProposal<TYPES>>,
    sender: TYPES::SignatureKey,
    cur_view: TYPES::Time,
    quorum_membership: Arc<TYPES::Membership>,
    timeout_membership: Arc<TYPES::Membership>,
) -> Result<()> {
    let view = proposal.data.get_view_number();
    ensure!(
        view > cur_view,
        "Proposal is from an older view {:?}",
        proposal.data.clone()
    );

    let view_leader_key = quorum_membership.get_leader(view);
    ensure!(
        view_leader_key == sender,
        "Leader key does not match key in proposal"
    );

    // Verify a timeout certificate OR a view sync certificate exists and is valid.
    if proposal.data.justify_qc.get_view_number() != view - 1 {
        if let Some(received_proposal_cert) = proposal.data.proposal_certificate.clone() {
            match received_proposal_cert {
                ViewChangeEvidence::Timeout(timeout_cert) => {
                    ensure!(timeout_cert.get_data().view == view - 1, "Timeout certificate for view {} was not for the immediately preceding view", *view);
                    ensure!(
                        timeout_cert.is_valid_cert(timeout_membership.as_ref()),
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
                        view_sync_cert.is_valid_cert(quorum_membership.as_ref()),
                        "Invalid view sync finalize cert provided"
                    );
                }
            }
        } else {
            bail!(
                "Quorum proposal for view {} needed a timeout or view sync certificate, but did not have one",
                *view);
        };
    }

    // Validate the upgrade certificate -- this is just a signature validation.
    // Note that we don't do anything with the certificate directly if this passes; it eventually gets stored as part of the leaf if nothing goes wrong.
    UpgradeCertificate::validate(&proposal.data.upgrade_certificate, &quorum_membership)?;

    Ok(())
}

/// If we've determined that we need to perform the liveness check, we use this function to check if a proposal
/// should also occur, then we vote regardless.
pub async fn perform_liveness_check_and_vote<TYPES: NodeType>(
    proposal: &Proposal<TYPES, QuorumProposal<TYPES>>,
    quorum_membership: Arc<TYPES::Membership>,
    public_key: TYPES::SignatureKey,
    high_qc: QuorumCertificate<TYPES>,
) -> Result<bool> {
    let new_view = proposal.data.view_number + 1;

    // This is for the case where we form a QC but have not yet seen the previous proposal ourselves
    Ok(quorum_membership.get_leader(new_view) == public_key
        && high_qc.view_number == proposal.data.get_view_number())
}
