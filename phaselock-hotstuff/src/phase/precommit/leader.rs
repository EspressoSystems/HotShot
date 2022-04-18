use crate::{
    phase::{Progress, UpdateCtx},
    ConsensusApi, Result,
};
use phaselock_types::{message::Vote, traits::node_implementation::NodeImplementation};

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct PreCommitLeader<const N: usize> {
    votes: Vec<Vote<N>>,
}

impl<const N: usize> PreCommitLeader<N> {
    pub(super) fn new(vote: Option<Vote<N>>) -> Self {
        Self {
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
