use std::time::Duration;

use async_compatibility_layer::{art, channel::UnboundedSender};
use async_std::task::JoinHandle;
use futures::{channel::mpsc, StreamExt};

use crate::network::ClientRequest;

/// Internal bootstrap events
pub enum InputEvent {
    /// Start bootstrap
    StartBootstrap,
    /// Bootstrap has finished
    BootstrapFinished,
}
/// Bootstrap task's state
pub struct DHTBootstrapTask {
    /// Task's receiver
    rx: mpsc::Receiver<InputEvent>,
    /// Task's sender
    network_tx: UnboundedSender<ClientRequest>,
    /// Field indicating progress state
    in_progress: bool,
}

impl DHTBootstrapTask {
    /// Run bootstrap task
    #[must_use]
    pub fn run(
        rx: mpsc::Receiver<InputEvent>,
        tx: UnboundedSender<ClientRequest>,
    ) -> JoinHandle<()> {
        art::async_spawn(async move {
            let state = Self {
                rx,
                network_tx: tx,
                in_progress: false,
            };
            state.run_loop().await;
        })
    }
    /// Task's loop
    async fn run_loop(mut self) {
        loop {
            tracing::debug!("looping bootstrap");
            if !self.in_progress {
                match art::async_timeout(Duration::from_secs(120), self.rx.next()).await {
                    Ok(maybe_event) => {
                        if let Some(InputEvent::StartBootstrap) = maybe_event {
                            tracing::debug!("Start bootstrap in bootstrap task");
                            self.bootstrap().await;
                        }
                    }
                    Err(_) => self.bootstrap().await,
                }
            } else if matches!(self.rx.next().await, Some(InputEvent::BootstrapFinished)) {
                tracing::debug!("Start bootstrap in bootstrap task after timout");
                self.in_progress = false;
            }
        }
    }
    /// Start bootstrap
    async fn bootstrap(&mut self) {
        self.in_progress = true;
        let _ = self.network_tx.send(ClientRequest::BeginBootstrap).await;
    }
}
