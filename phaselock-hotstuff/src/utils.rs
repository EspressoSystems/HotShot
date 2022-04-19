use crate::ConsensusApi;
use phaselock_types::{
    data::{Leaf, LeafHash, QuorumCertificate},
    traits::{node_implementation::NodeImplementation, storage::Storage},
};
use tracing::{error, trace, warn};

pub(super) async fn safe_node<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
    api: &A,
    known_qc: &QuorumCertificate<N>,
    leaf: &Leaf<I::Block, N>,
    new_qc: &QuorumCertificate<N>,
) -> bool {
    // new nodes can not be a genesis
    if new_qc.genesis {
        return false;
    }
    let view_number_valid = new_qc.view_number > known_qc.view_number;
    if !view_number_valid {
        return false;
    }

    // check if `new_qc` extends from `known_qc`
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

    // The original implementation claimed that this is `true`
    // However I feel like someone could construct a `leaf` with a parent of `[0; N]`, and bypass this check
    // So I changed it to `false`
    // TODO(vko): validate with nathan or joe if this is correct
    warn!(
        ?known_qc,
        ?leaf,
        ?new_qc,
        "Received a leaf but it has an invalid parent"
    );
    false
}
