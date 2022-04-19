use crate::{
    phase::{Progress, UpdateCtx},
    ConsensusApi, Result,
};
use phaselock_types::{message::Commit, traits::node_implementation::NodeImplementation};

#[derive(Debug)]
pub struct CommitReplica<const N: usize> {
    #[allow(dead_code)]
    commit: Option<Commit<N>>,
}

impl<const N: usize> CommitReplica<N> {
    pub fn new(commit: Option<Commit<N>>) -> Self {
        Self { commit }
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        _ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<()>> {
        todo!()
    }
}
