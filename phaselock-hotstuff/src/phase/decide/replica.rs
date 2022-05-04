use crate::{phase::UpdateCtx, utils, ConsensusApi, Result};
use phaselock_types::{
    data::QuorumCertificate, message::Decide, traits::node_implementation::NodeImplementation,
};

use super::Outcome;

/// The replica
#[derive(Debug)]
pub struct DecideReplica<const N: usize> {
    /// The QC that this round started with
    starting_qc: QuorumCertificate<N>,
}

impl<const N: usize> DecideReplica<N> {
    /// Create a new replica
    pub fn new(starting_qc: QuorumCertificate<N>) -> Self {
        Self { starting_qc }
    }

    /// Update the replica. This will:
    /// - Wait for a [`Decide`] message
    /// - Get the blocks and states that were decided on
    ///
    /// # Errors
    ///
    /// This will return an error if:
    /// - There was no QC in storage
    /// - `utils::walk_leaves` returns an error
    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
    ) -> Result<Option<Outcome<I, N>>> {
        let decide = if let Some(decide) = ctx.decide_message() {
            decide
        } else {
            return Ok(None);
        };

        let outcome = self.handle_decide(ctx, decide.clone()).await?;
        Ok(Some(outcome))
    }

    /// Handle an incoming [`Decide`] message
    ///
    /// # Errors
    ///
    /// Errors are described in the documentation of `update`
    #[tracing::instrument]
    async fn handle_decide<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
        decide: Decide<N>,
    ) -> Result<Outcome<I, N>> {
        // TODO: Walk from `storage().locked_qc()` instead
        let (blocks, states) =
            utils::walk_leaves(ctx.api, decide.leaf_hash, self.starting_qc.leaf_hash).await?;
        Ok(Outcome {
            blocks,
            states,
            decide,
        })
    }
}
