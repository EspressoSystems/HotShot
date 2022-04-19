#![allow(dead_code, unused_imports, unused_variables)]

mod leader;
mod replica;

use super::{err, precommit::PreCommitPhase, Phase, Progress, UpdateCtx};
use crate::{ConsensusApi, Result, TransactionLink, TransactionState};
use leader::PrepareLeader;
use phaselock_types::{
    data::{Leaf, QuorumCertificate, Stage},
    error::{FailedToBroadcastSnafu, PhaseLockError, StorageSnafu},
    message::{ConsensusMessage, Prepare, Vote},
    traits::{node_implementation::NodeImplementation, storage::Storage, BlockContents, State},
};
use replica::PrepareReplica;
use snafu::ResultExt;
use std::time::Instant;
use tracing::{debug, error, trace, warn};

#[derive(Debug)]
pub(crate) enum PreparePhase<const N: usize> {
    Leader(PrepareLeader<N>),
    Replica(PrepareReplica),
}

impl<const N: usize> PreparePhase<N> {
    pub(super) fn new(is_leader: bool) -> Self {
        if is_leader {
            Self::Leader(PrepareLeader::new())
        } else {
            Self::Replica(PrepareReplica::new())
        }
    }

    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<PreCommitPhase<I, N>>> {
        match (self, ctx.is_leader) {
            (Self::Leader(leader), true) => leader.update(ctx).await,
            (Self::Replica(replica), false) => replica.update(ctx).await,
            (this, _) => err(format!(
                "We're in {:?} but is_leader is {}",
                this, ctx.is_leader
            )),
        }
    }
}
