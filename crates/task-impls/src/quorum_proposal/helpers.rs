use std::sync::Arc;

use anyhow::{bail, ensure, Result};
use hotshot_types::{
    data::Leaf,
    simple_certificate::QuorumCertificate,
    traits::{election::Membership, node_implementation::NodeType},
};

pub(crate) async fn get_parent_leaf_and_state<TYPES: NodeType>(
    cur_view: TYPES::Time,
    view: TYPES::Time,
    quorum_membership: Arc<TYPES::Membership>,
    public_key: TYPES::SignatureKey,
    high_qc: &QuorumCertificate<TYPES>,
) -> Result<(Leaf<TYPES>, Arc<<TYPES as NodeType>::ValidatedState>)> {
    ensure!(
        quorum_membership.get_leader(view) == public_key,
        "Somehow we formed a QC but are not the leader for the next view {view:?}",
    );

    bail!("tmp")
}
