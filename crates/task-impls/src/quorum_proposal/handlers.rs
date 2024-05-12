use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{bail, ensure, Context, Result};
use hotshot_types::{
    data::QuorumProposal,
    event::LeafInfo,
    traits::{
        block_contents::BlockHeader,
        node_implementation::{NodeImplementation, NodeType},
        BlockPayload,
    },
    vote::HasViewNumber,
};

use super::QuorumProposalTaskState;

/// Ascends the leaf chain by traversing through the parent commitments of the proposal. We begin
/// by obtaining the parent view, and if we are in a chain (i.e. the next view from the parent is
/// one view newer), then we begin attempting to form the chain. From "HotStuff":
///
/// > When a node b* carries a QC that refers to a direct parent, i.e., b*.justify.node = b*.parent,
/// we say that it forms a One-Chain. Denote by b'' = b*.justify.node. Node b* forms a Two-Chain,
/// if in addition to forming a One-Chain, b''.justify.node = b''.parent.
/// It forms a Three-Chain, if b'' forms a Two-Chain.
///
/// We follow this exact logic to determine if we are able to reach a commit and a decide. A commit
/// is reached when we have a two chain, and a decide is reached when we have a three chain.
///
/// # Example
/// Suppose we have a decide for view 1, and we then move on to get undecided views 2, 3, and 4. Further,
/// suppose that our *next* proposal is for view 5, but this leader did not see info for view 4, so the
/// justify qc of the proposal points to view 3. This is fine, and the undecided chain now becomes
/// 2-3-5.
///
/// Assuming we continue with honest leaders, we then eventually could get a chain like: 2-3-5-6-7-8. This
/// will prompt a decide event to occur (this code), where the `proposal` is for view 8. Now, since the
/// lowest value in the 3-chain here would be 5 (excluding 8 since we only walk the parents), we begin at
/// the first link in the chain, and walk back through all undecided views, making our new anchor view 5,
/// and out new locked view will be 6.
///
/// Upon receipt then of a proposal for view 9, assuming it is valid, this entire process will repeat, and
/// the anchor view will be set to view 6, with the locked view as view 7.
async fn visit_leaf_chain<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    proposal: &QuorumProposal<TYPES>,
    task_state: &mut QuorumProposalTaskState<TYPES, I>,
) -> Result<()> {
    let proposal_view_number = proposal.get_view_number();
    let proposal_parent_view_number = proposal.justify_qc.get_view_number();

    // Are these views consecutive (1-chain)
    if proposal_parent_view_number + 1 == proposal_view_number {
        // We are in at least a 1-chain, so we start from here.
        let mut current_chain_length: usize = 1;

        // Get the state so we can traverse the chain to see if we have a 2 or 3 chain.
        let walk_start_view_number = proposal_parent_view_number;

        // The next view is the next view we're going to traverse, it is the validated state of the
        // parent of the proposal.
        let mut next_view = task_state
            .validated_states
            .get(&proposal_parent_view_number)
            .context(format!(
                "A leaf for view {walk_start_view_number:?} does not exist in the state map"
            ))?;

        // We need the leaf as well to ensure its state exists in its map, and to be used later once we
        // have a new chain.
        let mut next_leaf = next_view.get_leaf_commitment().context(format!(
            "View {walk_start_view_number:?} is a failed view, expected a successful leaf."
        ))?;

        // These are state variables to keep track of during our traversal.
        let mut decide_reached = false;
        let mut commit_reached = false;

        // The most recently seen view number (the view number of the last leaf we saw).
        let mut last_seen_view_number = walk_start_view_number;

        // Store the next locked view if there will be one (don't update the internal state yet
        // becuase some of the later ops can fail).
        let mut new_locked_view = task_state.locked_view;
        let mut new_decided_view = task_state.last_decided_view;
        let mut new_decide_qc = None;
        let mut leaf_views = Vec::new();
        let mut leaves_decided = Vec::new();
        let mut included_txns = HashSet::new();

        while let Some(leaf) = task_state.undecided_leaves.get(&next_leaf) {
            // These are all just checks to make sure we have what we need to proceed.
            let current_leaf_view_number = leaf.get_view_number();
            let leaf_state = task_state
                .validated_states
                .get(&current_leaf_view_number)
                .context(format!(
                    "View {current_leaf_view_number:?} does not exist in the state map"
                ))?;

            if let (Some(state), delta) = leaf_state.get_state_and_delta() {
                // Exit if we've reached the last anchor view.
                if current_leaf_view_number == task_state.last_decided_view {
                    bail!("We do not have a new chain extension")
                }

                // IMPORTANT: This is the logic from the paper, and is the most critical part of this function.
                if !decide_reached {
                    // Does this leaf extend the chain?
                    if last_seen_view_number == leaf.get_view_number() + 1 {
                        last_seen_view_number = leaf.get_view_number();
                        current_chain_length += 1;

                        // We've got a 2 chain, update the locked view.
                        if current_chain_length == 2 {
                            commit_reached = true;
                            new_locked_view = leaf.get_view_number();

                            // The next leaf in the chain, if there is one, is decided, so this
                            // leaf's justify_qc would become the QC for the decided chain.
                            new_decide_qc = Some(leaf.get_justify_qc().clone());
                        } else if current_chain_length == 3 {
                            // We've got the 3-chain, which means we can successfully decide on this.
                            new_decided_view = leaf.get_view_number();
                            decide_reached = true;
                        }
                    } else {
                        bail!("No new chain extension");
                    }
                }

                // If we got a 3-chain, we can start our state updates, garbage collection, etc
                if decide_reached {
                    let mut leaf = leaf.clone();
                    if leaf.get_view_number() == new_decided_view {
                        // TODO - Add consensus to task_state and upgrade metrics.
                        // consensus
                        //     .metrics
                        //     .last_synced_block_height
                        //     .set(usize::try_from(leaf.get_height()).unwrap_or(0));
                    }

                    // TODO - Upgrade certificates
                    // if let Some(cert) = leaf.get_upgrade_certificate() {
                    //     ensure!(
                    //         cert.data.decide_by >= proposal_view_number,
                    //         "Failed to decide an upgrade certificate in time. Ignoring."
                    //     );
                    //     task_state.decided_upgrade_cert = Some(cert.clone());
                    // }
                    // If the block payload is available for this leaf, include it in
                    // the leaf chain that we send to the client.
                    if let Some(encoded_txns) =
                        task_state.saved_payloads.get(&leaf.get_view_number())
                    {
                        let payload = BlockPayload::from_bytes(
                            encoded_txns,
                            leaf.get_block_header().metadata(),
                        );

                        leaf.fill_block_payload_unchecked(payload);
                    }

                    let vid_share = task_state
                        .vid_shares
                        .get(&leaf.get_view_number())
                        .unwrap_or(&HashMap::new())
                        .get(&task_state.public_key)
                        .cloned()
                        .map(|prop| prop.data);

                    // Add our data into a new `LeafInfo`
                    leaf_views.push(LeafInfo::new(
                        leaf.clone(),
                        Arc::clone(&state),
                        delta.clone(),
                        vid_share,
                    ));
                    leaves_decided.push(leaf.clone());
                    if let Some(ref payload) = leaf.get_block_payload() {
                        for txn in
                            payload.transaction_commitments(leaf.get_block_header().metadata())
                        {
                            included_txns.insert(txn);
                        }
                    }
                }
            } else {
                bail!(
                    "Validated state and delta do not exist for the leaf for view {current_leaf_view_number:?}"
                )
            };
        }
    }

    Ok(())
}

/// Handles the `QuorumProposalValidated` event.
pub(crate) async fn handle_quorum_proposal_validated<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
>(
    proposal: &QuorumProposal<TYPES>,
    task_state: &mut QuorumProposalTaskState<TYPES, I>,
) -> Result<()> {
    visit_leaf_chain(proposal, task_state).await?;

    Ok(())
}
