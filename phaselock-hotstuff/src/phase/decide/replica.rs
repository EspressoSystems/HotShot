use crate::{
    phase::{err, UpdateCtx},
    utils, ConsensusApi, Result,
};
use phaselock_types::{message::Decide, traits::node_implementation::NodeImplementation};

use super::Outcome;

#[derive(Debug)]
pub struct DecideReplica {}

impl DecideReplica {
    pub fn new() -> Self {
        Self {}
    }

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

    async fn handle_decide<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &self,
        ctx: &UpdateCtx<'_, I, A, N>,
        decide: Decide<N>,
    ) -> Result<Outcome<I, N>> {
        let old_qc = match ctx.get_newest_qc().await? {
            Some(qc) => qc,
            None => {
                return err("No QC in storage");
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
