use crate::{
    phase::{Progress, UpdateCtx},
    ConsensusApi, Result,
};
use phaselock_types::traits::node_implementation::NodeImplementation;

#[derive(Debug)]
pub struct PreCommitReplica {}

impl PreCommitReplica {
    pub fn new() -> Self {
        Self {}
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &mut self,
        _ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<()>> {
        todo!()
    }
}
