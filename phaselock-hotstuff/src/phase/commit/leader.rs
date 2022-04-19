use crate::{
    phase::{Progress, UpdateCtx},
    ConsensusApi, Result,
};
use phaselock_types::{
    message::{Commit, Vote},
    traits::node_implementation::NodeImplementation,
};

#[derive(Debug)]
pub(crate) struct CommitLeader<const N: usize> {
    #[allow(dead_code)]
    commit: Option<Commit<N>>,
    #[allow(dead_code)]
    votes: Vec<Vote<N>>,
}

impl<const N: usize> CommitLeader<N> {
    pub(super) fn new(commit: Option<Commit<N>>, vote: Option<Vote<N>>) -> Self {
        Self {
            commit,
            votes: vote.into_iter().collect(),
        }
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        _ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<()>> {
        todo!()
    }
}
