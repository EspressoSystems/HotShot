use async_lock::RwLock;
use std::sync::Arc;

use anyhow::{ensure, Context, Result};
use committable::Committable;
use hotshot_types::{
    consensus::Consensus,
    data::Leaf,
    simple_certificate::QuorumCertificate,
    traits::{election::Membership, node_implementation::NodeType},
    vote::HasViewNumber,
};
use tracing::debug;

pub(crate) async fn get_parent_leaf_and_state<TYPES: NodeType>(
    cur_view: TYPES::Time,
    view: TYPES::Time,
    quorum_membership: Arc<TYPES::Membership>,
    public_key: TYPES::SignatureKey,
    high_qc: &QuorumCertificate<TYPES>,
    consensus: Arc<RwLock<Consensus<TYPES>>>,
) -> Result<(Leaf<TYPES>, Arc<<TYPES as NodeType>::ValidatedState>)> {
    ensure!(
        quorum_membership.get_leader(view) == public_key,
        "Somehow we formed a QC but are not the leader for the next view {view:?}",
    );

    let parent_view_number = high_qc.get_view_number();
    let consensus = consensus.read().await;
    let parent_view = consensus.validated_state_map().get(&parent_view_number).context(
        format!("Couldn't find parent view in state map, waiting for replica to see proposal; parent_view_number: {parent_view_number:?}")
    )?;

    // Leaf hash in view inner does not match high qc hash - Why?
    let (leaf_commitment, state) = parent_view.get_leaf_and_state().context(
        format!("Parent of high QC points to a view without a proposal; parent_view_number: {parent_view_number:?}, parent_view {parent_view:?}")
    )?;

    let leaf = consensus
        .saved_leaves()
        .get(&leaf_commitment)
        .context("Failed to find high QC of parent")?;

    let reached_decided = leaf.get_view_number() == consensus.last_decided_view();
    let parent_leaf = leaf.clone();
    let original_parent_hash = parent_leaf.commit();
    let mut next_parent_hash = original_parent_hash;

    // Walk back until we find a decide
    if !reached_decided {
        debug!("We have not reached decide from view {:?}", cur_view);
        while let Some(next_parent_leaf) = consensus.saved_leaves().get(&next_parent_hash) {
            if next_parent_leaf.get_view_number() <= consensus.last_decided_view() {
                break;
            }
            next_parent_hash = next_parent_leaf.get_parent_commitment();
        }
        // TODO do some sort of sanity check on the view number that it matches decided
    }

    Ok((parent_leaf, Arc::clone(state)))
}
