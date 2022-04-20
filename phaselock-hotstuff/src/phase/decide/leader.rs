use crate::{
    phase::{Progress, UpdateCtx},
    ConsensusApi, Result,
};
use phaselock_types::{
    message::{Commit, CommitVote},
    traits::node_implementation::NodeImplementation,
};

#[derive(Debug)]
#[allow(dead_code)] // TODO(vko): cleanup
pub struct DecideLeader<const N: usize> {
    commit: Commit<N>,
    vote: Option<CommitVote<N>>,
}

impl<const N: usize> DecideLeader<N> {
    pub fn new(commit: Commit<N>, vote: Option<CommitVote<N>>) -> Self {
        Self { commit, vote }
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        _ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<()>> {
        todo!()
    }
}
