use crate::ConsensusApi;
use phaselock_types::message::ConsensusMessage;

#[derive(Debug)]
pub enum ReplicaPhase {}

impl ReplicaPhase {
    pub async fn progress<B, T, S, const N: usize>(
        &self,
        msg: ConsensusMessage<B, T, S, N>,
        api: &mut impl ConsensusApi<B, S, N>,
    ) -> ReplicaTransition {
        unimplemented!()
    }
}

pub enum ReplicaTransition {}
