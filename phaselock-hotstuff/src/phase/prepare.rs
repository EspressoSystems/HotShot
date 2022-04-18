#![allow(dead_code, unused_imports, unused_variables)]

use super::{err, Phase, Progress, UpdateCtx};
use crate::{ConsensusApi, Result, TransactionLink, TransactionState};
use phaselock_types::{
    data::{Leaf, QuorumCertificate, Stage},
    error::{FailedToBroadcastSnafu, PhaseLockError, StorageSnafu},
    message::{ConsensusMessage, Prepare, Vote},
    traits::{node_implementation::NodeImplementation, storage::Storage, BlockContents, State},
};
use snafu::ResultExt;
use std::time::Instant;
use tracing::{debug, error, trace, warn};

pub(super) struct PreparePhase<const N: usize> {
    votes: Vec<Vote<N>>,
}

impl<const N: usize> PreparePhase<N> {
    pub(super) fn new() -> Self {
        Self { votes: Vec::new() }
    }

    pub(super) fn add_vote(&mut self, vote: Vote<N>) {
        self.votes.push(vote);
    }
    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<()>> {
        if ctx.is_leader {
            self.update_leader(ctx).await
        } else {
            self.update_replica(ctx).await
        }
    }

    async fn update_leader<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<()>> {
        todo!()
    }

    async fn update_replica<I: NodeImplementation<N>, A: ConsensusApi<I, N>>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<()>> {
        todo!()
    }
}
