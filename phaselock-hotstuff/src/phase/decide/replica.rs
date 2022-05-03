use crate::{phase::UpdateCtx, utils, ConsensusApi, Result};
use phaselock_types::{message::Decide, traits::node_implementation::NodeImplementation};

use super::Outcome;

/// The replica
#[derive(Debug)]
pub struct DecideReplica {}

impl DecideReplica {
    /// Create a new replica
    pub fn new() -> Self {
        Self {}
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
    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
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
    async fn handle_decide<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
        decide: Decide<N>,
    ) -> Result<Outcome<I, N>> {
        let old_qc = match ctx.get_newest_qc().await? {
            Some(qc) => qc,
            None => {
                return utils::err("No QC in storage");
            }
        };
        let (blocks, states) =
            utils::walk_leaves(ctx.api, decide.leaf_hash, old_qc.leaf_hash).await?;
        Ok(Outcome {
            blocks,
            states,
            decide,
        })
    }
}
