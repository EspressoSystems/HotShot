// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use anyhow::Result;
use async_broadcast::{Receiver, Sender};
use async_trait::async_trait;
use chrono::Utc;
use hotshot_task::task::TaskState;
use hotshot_types::traits::node_implementation::NodeType;

use crate::events::{HotShotEvent, HotShotTaskCompleted};

/// Health event task, recieve heart beats from other tasks
pub struct HealthCheckTaskState<TYPES: NodeType> {
    /// Node id
    pub node_id: u64,
    /// Map of the task id to timestamp of last heartbeat
    pub task_id_last_seen: HashMap<usize, i64>,
    /// phantom
    pub _phantom: PhantomData<TYPES>,
}

impl<TYPES: NodeType> HealthCheckTaskState<TYPES> {
    /// Create a new instance of task state with task ids pre populated
    #[must_use]
    pub fn new(node_id: u64, total_tasks: usize) -> Self {
        let time = Utc::now().timestamp();
        let mut task_id_last_seen: HashMap<usize, i64> = HashMap::new();
        for task_id in 0..total_tasks {
            task_id_last_seen.insert(task_id, time);
        }

        HealthCheckTaskState {
            node_id,
            task_id_last_seen,
            _phantom: std::marker::PhantomData,
        }
    }
    /// Handles only HeartBeats and updates the timestamp for a task
    pub fn handle(
        &mut self,
        event: &Arc<HotShotEvent<TYPES>>,
        _sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Option<HotShotTaskCompleted> {
        match event.as_ref() {
            HotShotEvent::HeartBeat(task_id) => {
                if self.node_id == 0 {
                    tracing::error!("heart beat recieved {} {}", task_id, self.node_id);
                }
                self.task_id_last_seen
                    .insert(*task_id, Utc::now().timestamp());
            }
            HotShotEvent::Shutdown => {
                return Some(HotShotTaskCompleted);
            }
            _ => {}
        }
        None
    }
}

#[async_trait]
impl<TYPES: NodeType> TaskState for HealthCheckTaskState<TYPES> {
    type Event = HotShotEvent<TYPES>;

    async fn handle_event(
        &mut self,
        event: Arc<Self::Event>,
        sender: &Sender<Arc<Self::Event>>,
        _receiver: &Receiver<Arc<Self::Event>>,
    ) -> Result<()> {
        self.handle(&event, sender);

        Ok(())
    }

    async fn cancel_subtasks(&mut self) {}

    async fn periodic_task(&self, _task_id: usize, _sender: &Sender<Arc<Self::Event>>) {
        let current_time = Utc::now().timestamp();
        let max_time_no_heartbeat_in_seconds = 6;

        for (task_id, timestamp) in &self.task_id_last_seen {
            let time_diff = current_time - timestamp;
            if time_diff > max_time_no_heartbeat_in_seconds {
                tracing::error!(
                    "Task ID {} has not been updated for more than {} seconds",
                    task_id,
                    max_time_no_heartbeat_in_seconds
                );
            }
        }
    }
}
