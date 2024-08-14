// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::sync::Arc;

use anyhow::Result;
use async_broadcast::{Receiver, Sender};
use async_trait::async_trait;
use hotshot_task::task::TaskState;
use hotshot_types::traits::node_implementation::NodeType;

use crate::{
    events::{HotShotEvent, HotShotTaskCompleted},
    helpers::broadcast_event,
};

pub enum ConsensusLifetimeTasks {
    Network,
    Da,
    Res,
}

/// health event
pub struct HealthCheckTaskState<TYPES: NodeType> {
    /// current view
    pub view: TYPES::Time,
    /// last view the health check was broadcasted
    pub last_health_check_view: TYPES::Time,
    /// how many views until we broadcast again
    pub broadcast_health_event: u64,
}

impl<TYPES: NodeType> HealthCheckTaskState<TYPES> {
    /// Handles all events, storing them to the private state
    pub async fn handle(
        &mut self,
        event: &Arc<HotShotEvent<TYPES>>,
        sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Option<HotShotTaskCompleted> {
        match event.as_ref() {
            HotShotEvent::ViewChange(view) => {
                let view = *view;
                if view > self.view {
                    self.view = view;
                }
                if *self.view - *self.last_health_check_view > self.broadcast_health_event {
                    tracing::error!(
                        "sending health check probe: {:?} {:?} {:?}",
                        self.view,
                        self.last_health_check_view,
                        sender
                    );
                    broadcast_event(Arc::new(HotShotEvent::HealthCheckProbe(self.view)), sender)
                        .await;

                    self.last_health_check_view = self.view;
                    self.broadcast_health_event = 5000;
                }
            }
            HotShotEvent::HealthCheckResponse => {
                tracing::error!("probe recieved");
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
}
