use crate::{ConsensusApi, Result, TransactionState};
use phaselock_types::{
    data::{Leaf, LeafHash, QuorumCertificate},
    error::StorageSnafu,
    traits::{node_implementation::NodeImplementation, storage::Storage, BlockContents, State},
};
use snafu::ResultExt;
use tracing::{debug, error, trace, warn};

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

pub(crate) fn append_transactions<I: NodeImplementation<N>, const N: usize>(
    transactions: Vec<TransactionState<I, N>>,
    block: &mut I::Block,
    state: &I::State,
) -> Vec<TransactionState<I, N>> {
    let mut added_transactions = Vec::new();
    for transaction in transactions {
        let tx = &transaction.transaction;
        // Make sure the transaction is valid given the current state,
        // otherwise, discard it
        let new_block = block.add_transaction_raw(tx);
        match new_block {
            Ok(new_block) => {
                if state.validate_block(&new_block) {
                    *block = new_block;
                    debug!(?tx, "Added transaction to block");
                    added_transactions.push(transaction);
                } else {
                    // TODO: `state.append` could change our state.
                    // we should probably make `validate_block` return this error.
                    let err = state.append(&new_block).unwrap_err();
                    warn!(?tx, ?err, "Invalid transaction rejected");
                }
            }
            Err(e) => warn!(?e, ?tx, "Invalid transaction rejected"),
        }
    }
    added_transactions
}
