//! Utility functions

use crate::{ConsensusApi, Result};
use phaselock_types::{
    data::{Leaf, LeafHash, QuorumCertificate},
    error::StorageSnafu,
    traits::{node_implementation::NodeImplementation, storage::Storage, State},
};
use snafu::ResultExt;
use tracing::{debug, error, info, trace, warn};

/// Check if the given `new_qc` is considered a "safe node".
///
/// `known_qc` is a QC that is known to be good (probably loaded from `api.storage()`)
///
/// If any storage-based error occurs, this will return `false`
pub(crate) async fn safe_node<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
    api: &A,
    known_qc: &QuorumCertificate<N>,
    leaf: &Leaf<I::Block, N>,
    new_qc: &QuorumCertificate<N>,
) -> bool {
    // new nodes can not be a genesis
    if new_qc.genesis {
        return false;
    }
    // safeNode predicate. The safeNode predicate is a core ingredient of the protocol. It examines a proposal message
    // m carrying a QC justification m.justify, and determines whether m.node is safe to accept. The safety rule to accept
    // a proposal is the branch of m.node extends from the currently locked node locked QC .node[2]. On the other hand, the
    // liveness rule is the replica will accept m if m.justify has a higher view than the current locked QC[1] . The predicate is
    // true as long as either one of two rules holds.

    // [1]: if m[..] has a higher view than the current locked QC
    let view_number_valid = new_qc.view_number > known_qc.view_number;
    if view_number_valid {
        return true;
    }

    // [2] the branch of m.node extends from the currently locked node
    let valid_leaf_hash = known_qc.leaf_hash;
    let mut parent = leaf.parent;

    while parent != LeafHash::from_array([0_u8; N]) {
        if parent == valid_leaf_hash {
            trace!(?parent, ?leaf, "Leaf extends from");
            return true;
        }
        let result = api.storage().get_leaf(&parent).await;
        if let Ok(Some(next_parent)) = result {
            parent = next_parent.parent;
        } else {
            error!(?result, ?parent, "Parent leaf does not extend from node");
            return false;
        }
    }

    // If the node has no parent, then we return `true`:
    // `the way the protocol prevents the issue you are worried about is that safeNode is actually run against a proposed
    //  leaf's justify QC, and the leaf's descent from its justify QC is checked before running safeNode, so in order
    // for this issue to crop up, you would have needed 2/3's of the network to have already voted for a block that
    // doesn't descend from anything in history, and there's no way to bootstrap that situation unless the saftey bounds
    // are violated`
    // https://github.com/EspressoSystems/phaselock/pull/121#discussion_r856538610
    info!(?known_qc, ?leaf, ?new_qc, "new QC has an empty parent");
    true
}

/// Walk the given `walk_leaf` up until we reach `old_leaf_hash`. Existing leafs will be loaded from `api.storage().get_leaf(&hash)`.
///
/// # Errors
///
/// Will return an error if the underlying storage returns an error.
pub(crate) async fn walk_leaves<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
    api: &A,
    mut walk_leaf: LeafHash<N>,
    old_leaf_hash: LeafHash<N>,
) -> Result<(Vec<I::Block>, Vec<I::State>)> {
    let mut blocks = vec![];
    let mut states = vec![];
    while walk_leaf != old_leaf_hash {
        debug!(?walk_leaf, "Looping");
        let leaf = if let Some(x) = api
            .storage()
            .get_leaf(&walk_leaf)
            .await
            .context(StorageSnafu)?
        {
            x
        } else {
            warn!(?walk_leaf, "Parent did not exist in store");
            break;
        };
        let state = if let Some(x) = api
            .storage()
            .get_state(&walk_leaf)
            .await
            .context(StorageSnafu)?
        {
            x
        } else {
            warn!(?walk_leaf, "Parent did not exist in store");
            break;
        };
        blocks.push(leaf.item);
        states.push(state);
        walk_leaf = leaf.parent;
    }
    for state in &states {
        state.on_commit();
    }
    Ok((blocks, states))
}
