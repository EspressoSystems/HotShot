use anyhow::Result;
use hotshot_types::{
    data::QuorumProposal,
    traits::node_implementation::{NodeImplementation, NodeType},
};
use tracing::{info, warn};

use super::QuorumProposalTaskState;

async fn visit_leaf_chain() -> Result<()> {}

/// Handles the `QuorumProposalValidated` event.
pub(crate) async fn handle_quorum_proposal_validated<TYPES: NodeType, I: NodeImplementation>(
    proposal: &QuorumProposal<TYPES>,
    task_state: &mut QuorumProposalTaskState<TYPES, I>,
) -> Result<()> {
    visit_leaf_chain().await?;

    Ok(())
}
