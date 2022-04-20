use crate::{
    phase::{Progress, UpdateCtx},
    ConsensusApi, Result,
};
use phaselock_types::traits::node_implementation::NodeImplementation;

#[derive(Debug)]
pub struct DecideLeader {
    #[allow(dead_code)] // TODO(vko): clean this up
    already_validated: bool,
}

impl DecideLeader {
    pub fn new(already_validated: bool) -> Self {
        Self { already_validated }
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &mut self,
        _ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<()>> {
        todo!()
    }
}
