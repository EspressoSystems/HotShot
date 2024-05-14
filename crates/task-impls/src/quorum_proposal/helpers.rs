use std::sync::Arc;

use anyhow::{ensure, Context, Result};
use committable::Committable;
use hotshot_types::{
    data::Leaf,
    simple_certificate::QuorumCertificate,
    traits::{election::Membership, node_implementation::NodeType},
    vote::HasViewNumber,
};
use tracing::debug;

use super::dependency_handle::ProposalDependencyHandle;

/// Gets the parent leaf and state from the parent of the proposal.
///
/// # Errors
/// Can throw an error if we are not the lader, if we do not have the parent view
/// of the high qc stored in memory, or if the parent of the high qc does not have
/// a proposal.
pub(crate) async fn get_parent_leaf_and_state<TYPES: NodeType>(
    latest_proposed_view: TYPES::Time,
    view_number: TYPES::Time,
    high_qc: &QuorumCertificate<TYPES>,
    task_state: &ProposalDependencyHandle<TYPES>,
) -> Result<(Leaf<TYPES>, Arc<<TYPES as NodeType>::ValidatedState>)> {
    ensure!(
        task_state.quorum_membership.get_leader(view_number) == task_state.public_key,
        "Somehow we formed a QC but are not the leader for the next view {view_number:?}",
    );

    let parent_view_number = high_qc.get_view_number();

    let consensus_reader = task_state.consensus.read().await;
    let parent_view = consensus_reader.validated_state_map().get(&parent_view_number).context(
        format!("Couldn't find parent view in state map, waiting for replica to see proposal; parent_view_number: {}", *parent_view_number)
    )?;

    let (leaf_commitment, state) = parent_view.get_leaf_and_state().context(
        format!("Parent of high QC points to a view without a proposal; parent_view_number: {parent_view_number:?}, parent_view {parent_view:?}")
    )?;

    let leaf = consensus_reader
        .saved_leaves()
        .get(&leaf_commitment)
        .context("Failed to find high QC of parent")?;

    let reached_decided = leaf.get_view_number() == consensus_reader.last_decided_view();
    let parent_leaf = leaf.clone();
    let original_parent_hash = parent_leaf.commit();
    let mut next_parent_hash = original_parent_hash;

    // Walk back until we find a decide
    if !reached_decided {
        debug!(
            "We have not reached decide from view {:?}",
            latest_proposed_view
        );
        while let Some(next_parent_leaf) = consensus_reader.saved_leaves().get(&next_parent_hash) {
            if next_parent_leaf.get_view_number() <= consensus_reader.last_decided_view() {
                break;
            }
            next_parent_hash = next_parent_leaf.get_parent_commitment();
        }
        // TODO do some sort of sanity check on the view number that it matches decided
    }

    Ok((parent_leaf, Arc::clone(state)))
}
