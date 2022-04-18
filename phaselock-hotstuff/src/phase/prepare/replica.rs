use crate::phase::precommit::PreCommitPhase;
use crate::phase::{err, Phase, Progress, UpdateCtx};
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

#[derive(Debug)]
pub(crate) struct PrepareReplica {}

impl PrepareReplica {
    pub(super) fn new() -> Self {
        Self {}
    }
    pub(super) async fn update<I: NodeImplementation<N>, A: ConsensusApi<I, N>, const N: usize>(
        &mut self,
        ctx: &mut UpdateCtx<'_, I, A, N>,
    ) -> Result<Progress<PreCommitPhase>> {
        todo!()
    }
}
