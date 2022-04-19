use crate::{
    phase::{commit::CommitPhase, Progress, UpdateCtx},
    ConsensusApi, Result,
};
use phaselock_types::{message::Prepare, traits::node_implementation::NodeImplementation};

#[derive(Debug)]
pub struct PreCommitReplica<I: NodeImplementation<N>, const N: usize> {
    #[allow(dead_code)]
    prepare: Option<Prepare<I::Block, I::State, N>>,
}

impl<I: NodeImplementation<N>, const N: usize> PreCommitReplica<I, N> {
    pub fn new(prepare: Option<Prepare<I::Block, I::State, N>>) -> Self {
        Self { prepare }
    }

    pub(super) async fn update<A: ConsensusApi<I, N>>(
        &mut self,
        _ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<CommitPhase<N>>> {
        todo!()
    }
}
