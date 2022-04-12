use super::Ctx;
use crate::{ConsensusApi, ConsensusMessage, OptionUtils, Result};
use phaselock_types::{
    data::QuorumCertificate,
    error::StorageSnafu,
    message::NewView,
    traits::{node_implementation::NodeImplementation, storage::Storage, State},
};
use snafu::ResultExt;
use tracing::{debug, instrument, warn};

#[derive(Debug, Clone)]
pub struct Prepare<const N: usize> {
    current_view: u64,
    received_qcs: Vec<QuorumCertificate<N>>,
}

pub(super) enum PrepareTransition<I: NodeImplementation<N>, const N: usize> {
    /// Stay in this state, do not transition.
    Stay,

    /// Go to CollectTransactions phase
    CollectTransactions {
        ctx: Ctx<I, N>,
        high_qc: QuorumCertificate<N>,
    },
}

impl<const N: usize> Prepare<N> {
    pub(super) fn new(current_view: u64) -> Self {
        Self {
            current_view,
            received_qcs: Vec::new(),
        }
    }

    #[instrument(skip(api))]
    pub(super) async fn step<I: NodeImplementation<N>>(
        &mut self,
        msg: ConsensusMessage<'_, I, N>,
        api: &mut impl ConsensusApi<I, N>,
    ) -> Result<PrepareTransition<I, N>> {
        match msg {
            ConsensusMessage::NewView(view) => self.handle_new_view(view, api).await,
            msg => {
                warn!(?msg, "Expected NewView message, ignoring this.");
                Ok(PrepareTransition::Stay)
            }
        }
    }

    async fn handle_new_view<I: NodeImplementation<N>>(
        &mut self,
        view: NewView<N>,
        api: &mut impl ConsensusApi<I, N>,
    ) -> Result<PrepareTransition<I, N>> {
        // The protocol for a new leader starts by collecting new-view messages from (n − f ) replicas. [1]
        // The new-view message is sent by a replica as it transitions into viewNumber (including the first view) and carries
        // the highest prepareQC that the replica received (⊥ if none), as described below.
        //
        // The leader processes these messages in order to select a branch that has the highest preceding view in which
        // a prepareQC was formed. The leader selects the prepareQC with the highest view, denoted highQC , among the
        // new-view messages. [2] Because highQC is the highest among (n − f ) replicas, no higher view could have reached a
        // commit decision. The branch led by highQC .node is therefore safe.
        //
        // [..]
        //
        // The leader uses the `createLeaf` method to extend the tail of highQC .node with a new proposal. The method
        // creates a new leaf node as a child and embeds a digest of the parent in the child node. The leader then sends the new
        // node in a prepare message to all other replicas. The proposal carries highQC for safety justification.

        // [1] collect `n-f` new-view messages
        self.received_qcs.push(view.justify.clone());
        if self.received_qcs.len() < api.threshold().get() as usize {
            return Ok(PrepareTransition::Stay);
        }

        // [2] select HighQC
        let high_qc = self
            .received_qcs
            .iter()
            .max_by_key(|qc| qc.view_number)
            // `api.threshold() is NonZero so this `unwrap` should never be triggered
            .unwrap()
            .clone();

        debug!(?high_qc);
        if high_qc.view_number != self.current_view {
            warn!(
                "high_qc view number is {}, but we are at view number {}, this could cause issues",
                high_qc.view_number, self.current_view
            );
        }

        // Get leaf and state
        let storage = api.storage();
        let leaf = storage
            .get_leaf_by_block(&high_qc.block_hash)
            .await
            .context(StorageSnafu)?
            .or_not_found(&high_qc.block_hash)?;

        let leaf_hash = leaf.hash();
        let state = storage
            .get_state(&leaf_hash)
            .await
            .context(StorageSnafu)?
            .or_not_found(&leaf_hash)?;

        // Get an (incomplete) new block and start collecting transactions
        let block = state.next_block();
        Ok(PrepareTransition::CollectTransactions {
            ctx: Ctx {
                current_view: self.current_view,

                block,
                state: state.clone(),
                leaf: leaf.clone(),

                committed_state: state,
                committed_leaf: leaf,
            },
            high_qc,
        })
    }
}
