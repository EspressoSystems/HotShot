// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{
    collections::{hash_map::Entry, HashMap},
    marker::PhantomData,
    sync::Arc,
};

use anyhow::Result;
use async_broadcast::{Receiver, Sender};
use async_lock::Mutex;
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
    pub task_ids_heartbeat: Mutex<HashMap<String, i64>>,
    /// phantom
    pub _phantom: PhantomData<TYPES>,
}

impl<TYPES: NodeType> HealthCheckTaskState<TYPES> {
    /// Create a new instance of task state with task ids pre populated
    #[must_use]
    pub fn new(node_id: u64, task_ids: Vec<String>) -> Self {
        let time = Utc::now().timestamp();
        let mut task_ids_heartbeat: HashMap<String, i64> = HashMap::new();
        for task_id in task_ids {
            task_ids_heartbeat.insert(task_id, time);
        }

        HealthCheckTaskState {
            node_id,
            task_ids_heartbeat: Mutex::new(task_ids_heartbeat),
            _phantom: std::marker::PhantomData,
        }
    }
    /// Handles only HeartBeats and updates the timestamp for a task
    pub async fn handle(
        &mut self,
        event: &Arc<HotShotEvent<TYPES>>,
        _sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Option<HotShotTaskCompleted> {
        match event.as_ref() {
            HotShotEvent::HeartBeat(task_id) => {
                if self.node_id == 0 {
                    tracing::error!("heart beat recieved {} {}", task_id, self.node_id);
                }

                let mut task_ids_heartbeat = self.task_ids_heartbeat.lock().await;
                match task_ids_heartbeat.entry(task_id.clone()) {
                    Entry::Occupied(mut heartbeat_timestamp) => {
                        *heartbeat_timestamp.get_mut() = Utc::now().timestamp();
                    }
                    Entry::Vacant(_) => {
                        // TODO: is this expected? how to handle?
                    }
                }
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
        self.handle(&event, sender).await;

        Ok(())
    }

    async fn cancel_subtasks(&mut self) {}

    async fn periodic_task(&self, _task_id: String, _sender: &Sender<Arc<Self::Event>>) {
        let current_time = Utc::now().timestamp();
        let max_time_no_heartbeat_in_seconds = 30;

        let task_ids_heartbeat = self.task_ids_heartbeat.lock().await;
        for (task_id, heartbeat_timestamp) in task_ids_heartbeat.iter() {
            let last_heartbeat_time_diff = current_time - heartbeat_timestamp;
            if last_heartbeat_time_diff > max_time_no_heartbeat_in_seconds {
                tracing::error!(
                    "Node Id {} has no recieved a heartbeat for task id {} for {} seconds",
                    self.node_id,
                    task_id,
                    max_time_no_heartbeat_in_seconds
                );

                // TODO: Do we want to remove the task from our heartbeat cache after a few minutes to stop logging?
            }
        }
    }

    fn get_task_name(&self) -> String {
        "HealthCheckTask".to_string()
    }
}
