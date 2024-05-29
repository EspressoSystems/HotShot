use std::{collections::HashSet, marker::PhantomData, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use hotshot_task_impls::events::HotShotEvent;
use hotshot_types::traits::node_implementation::{NodeType, TestableNodeImplementation};
use snafu::Snafu;

use crate::test_task::{TestResult, TestTaskState};

/// `ViewSync` Task error
#[derive(Snafu, Debug, Clone)]
pub struct ViewSyncTaskErr {
    /// set of node ids that hit view sync
    hit_view_sync: HashSet<usize>,
}

/// `ViewSync` task state
pub struct ViewSyncTask<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    /// nodes that hit view sync
    pub(crate) hit_view_sync: HashSet<usize>,
    /// properties of task
    pub(crate) description: ViewSyncTaskDescription,
    /// Phantom data for TYPES and I
    pub(crate) _pd: PhantomData<(TYPES, I)>,
}

#[async_trait]
impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> TestTaskState
    for ViewSyncTask<TYPES, I>
{
    type Event = Arc<HotShotEvent<TYPES>>;

    /// Handles an event from one of multiple receivers.
    async fn handle_event(&mut self, (event, id): (Self::Event, usize)) -> Result<()> {
        match event.as_ref() {
            // all the view sync events
            HotShotEvent::ViewSyncTimeout(_, _, _)
            | HotShotEvent::ViewSyncPreCommitVoteRecv(_)
            | HotShotEvent::ViewSyncCommitVoteRecv(_)
            | HotShotEvent::ViewSyncFinalizeVoteRecv(_)
            | HotShotEvent::ViewSyncPreCommitVoteSend(_)
            | HotShotEvent::ViewSyncCommitVoteSend(_)
            | HotShotEvent::ViewSyncFinalizeVoteSend(_)
            | HotShotEvent::ViewSyncPreCommitCertificate2Recv(_)
            | HotShotEvent::ViewSyncCommitCertificate2Recv(_)
            | HotShotEvent::ViewSyncFinalizeCertificate2Recv(_)
            | HotShotEvent::ViewSyncPreCommitCertificate2Send(_, _)
            | HotShotEvent::ViewSyncCommitCertificate2Send(_, _)
            | HotShotEvent::ViewSyncFinalizeCertificate2Send(_, _)
            | HotShotEvent::ViewSyncTrigger(_) => {
                self.hit_view_sync.insert(id);
            }
            _ => (),
        }

        Ok(())
    }

    fn check(&self) -> TestResult {
        match self.description.clone() {
            ViewSyncTaskDescription::Threshold(min, max) => {
                let num_hits = self.hit_view_sync.len();
                if min <= num_hits && num_hits <= max {
                    TestResult::Pass
                } else {
                    TestResult::Fail(Box::new(ViewSyncTaskErr {
                        hit_view_sync: self.hit_view_sync.clone(),
                    }))
                }
            }
        }
    }
}

/// enum desecribing whether a node should hit view sync
#[derive(Clone, Debug, Copy)]
pub enum ShouldHitViewSync {
    /// the node should hit view sync
    Yes,
    /// the node should not hit view sync
    No,
    /// don't care if the node should hit view sync
    Ignore,
}

/// Description for a view sync task.
#[derive(Clone, Debug)]
pub enum ViewSyncTaskDescription {
    /// (min, max) number nodes that may hit view sync, inclusive
    Threshold(usize, usize),
}
