use crate::{
    phase::{Progress, UpdateCtx},
    ConsensusApi, Result,
};
use phaselock_types::{message::CommitVote, traits::node_implementation::NodeImplementation};

#[derive(Debug)]
pub struct DecideLeader<const N: usize> {
    #[allow(dead_code)] // TODO(vko): clean this up
    vote: Option<CommitVote<N>>,
    #[allow(dead_code)] // TODO(vko): clean this up
    already_validated: bool,
}

impl<const N: usize> DecideLeader<N> {
    pub fn new(vote: Option<CommitVote<N>>, already_validated: bool) -> Self {
        Self {
            vote,
            already_validated,
        }
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        _ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<()>> {
        todo!()
    }
}
