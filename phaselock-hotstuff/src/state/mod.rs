mod leader;
// mod replica;

pub use leader::*;
// pub use replica::*;

use crate::{ConsensusApi, ConsensusMessage, Result};
use phaselock_types::traits::node_implementation::NodeImplementation;

#[derive(Debug, Clone)]
#[allow(dead_code)] // TODO(vko): Prune unused variables
pub(crate) enum State<I: NodeImplementation<N>, const N: usize> {
    BeforeRound { view_number: u64 },
    Leader(LeaderPhase<I, N>),
    AfterRound { current_view: u64 },
}

impl<I: NodeImplementation<N>, const N: usize> Default for State<I, N> {
    fn default() -> Self {
        Self::BeforeRound { view_number: 0 }
    }
}

impl<I: NodeImplementation<N>, const N: usize> State<I, N> {
    pub async fn step(
        &mut self,
        msg: ConsensusMessage<'_, I, N>,
        api: &mut impl ConsensusApi<I, N>,
    ) -> Result {
        if let Self::BeforeRound { view_number } = self {
            if api.is_leader_this_round(*view_number).await {
                *self = Self::Leader(LeaderPhase::new(*view_number));
            } else {
                unimplemented!();
            }
        }

        match self {
            Self::BeforeRound { .. } => {
                unreachable!()
            }
            Self::Leader(leader) => match leader.step(msg, api).await? {
                LeaderTransition::Stay => Ok(()),
                LeaderTransition::End { current_view } => {
                    *self = Self::AfterRound { current_view };
                    Ok(())
                }
            },
            Self::AfterRound { .. } => {
                panic!("Event received in `AfterRound`");
            }
        }
    }
}
