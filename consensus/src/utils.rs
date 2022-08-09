//! Utility functions

use crate::{ConsensusApi, Result};
use hotshot_types::{
    data::{Leaf, LeafHash, QuorumCertificate},
    error::{HotShotError, StorageSnafu},
    traits::{node_implementation::NodeImplementation, storage::Storage, State},
};
use snafu::ResultExt;
use tracing::{debug, info, trace, warn};

/// Checks if a leaf descends from another leaf
pub(crate) async fn leaf_descends_from<
    I: NodeImplementation<N>,
    A: ConsensusApi<I, N>,
    const N: usize,
>(
    api: &A,
    leaf: &Leaf<I::Block, I::State, N>,
    ancestor_hash: LeafHash<N>,
) -> bool {
    let leaf_hash = leaf.hash();
    // Check if the leaf is the ancestor
    if leaf_hash == ancestor_hash {
        return true;
    }
    // Check that the ancestor exists in storage
    if api
        .storage()
        .get_leaf(&ancestor_hash)
        .await
        .unwrap_or(None)
        .is_none()
    {
        warn!(
            ?leaf_hash,
            ?ancestor_hash,
            "Ancestor in descent check was not present in storage"
        );
        return false;
    }
    // Make sure that the leaf under test has a valid parent
    let empty_hash = LeafHash::from_array([0_u8; N]);
    if leaf.parent == empty_hash {
        warn!(?leaf, "Incoming leaf has an empty parent in descent check");
        return false;
    }
    // Walk the parent list until we either hit nothing, a missing leaf
    let mut parent_hash = leaf.parent;
    while parent_hash != empty_hash {
        trace!(?parent_hash);
        // Ancestor is, in fact, our ancestor
        if parent_hash == ancestor_hash {
            return true;
        }
        // get the parent from storage
        if let Some(parent) = api.storage().get_leaf(&parent_hash).await.unwrap_or(None) {
            parent_hash = parent.parent;
        } else {
            warn!(
                ?leaf,
                ?ancestor_hash,
                ?parent_hash,
                "Ancestor was not in storage"
            );
            return false;
        }
    }
    debug!(
        ?leaf,
        ?ancestor_hash,
        "Leaf implicitly failed descent check"
    );
    false
}

/// Check if the given `new_qc` is considered a "safe node".
///
/// `locked_qc` is known to be good (probably loaded from `api.storage()`)
///
/// If any storage-based error occurs, this will return `false`
pub(crate) async fn validate_against_locked_qc<
    I: NodeImplementation<N>,
    A: ConsensusApi<I, N>,
    const N: usize,
>(
    api: &A,
    locked_qc: &QuorumCertificate<N>,
    new_leaf: &Leaf<I::Block, I::State, N>,
    high_qc: &QuorumCertificate<N>,
) -> bool {
    // Check to make sure the leaf actually descends from its high_qc
    if !leaf_descends_from(api, new_leaf, high_qc.leaf_hash).await {
        warn!(
            ?new_leaf,
            ?high_qc,
            "Leaf does not descend from its high_qc"
        );
        return false;
    }

    // Liveness Rule:
    //
    // If the view number of the new_leaf's high_qc is higher than our current
    // locked_qc, then the leaf is okay to build off of.
    let view_number_valid = high_qc.view_number > locked_qc.view_number;
    if view_number_valid {
        info!(?new_leaf, ?high_qc, "Leaf being approved on liveness rule");
        return true;
    }

    // Saftey rule:
    //
    // If the leaf descends from the node referenced by our current locked_qc,
    // then it is safe to build off of
    //
    // Directly return the result of the descent check here, as we have also
    // failed the liveness check by this point
    leaf_descends_from(api, new_leaf, locked_qc.leaf_hash).await
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
    let mut blocks: Vec<I::Block> = vec![];
    let mut states: Vec<I::State> = vec![];
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
        blocks.push(leaf.deltas);
        states.push(state);
        walk_leaf = leaf.parent;
    }
    for state in &states {
        state.on_commit();
    }
    Ok((blocks, states))
}

/// Return an `HotShotError::InvalidState` error with the given text.
pub(crate) fn err<T, S>(e: S) -> Result<T>
where
    S: Into<String>,
{
    Err(HotShotError::InvalidState { context: e.into() })
}
