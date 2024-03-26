use std::time::Duration;

use async_compatibility_layer::{art, channel::UnboundedSender};
use futures::{channel::mpsc, StreamExt};

use crate::network::ClientRequest;

pub enum InputEvent {
    StartBootstrap,
    BootstrapFinished,
}
pub struct DHTBootstrapTask {
    rx: mpsc::Receiver<InputEvent>,
    network_tx: UnboundedSender<ClientRequest>,
    in_progress: bool,
}

impl DHTBootstrapTask {
    pub fn run(rx: mpsc::Receiver<InputEvent>, tx: UnboundedSender<ClientRequest>) {
        art::async_spawn(async move {
            let state = Self {
                rx,
                network_tx: tx,
                in_progress: false,
            };
            state.run_loop().await;
        });
    }
    async fn run_loop(mut self) {
        loop {
            if !self.in_progress {
                match art::async_timeout(Duration::from_secs(120), self.rx.next()).await {
                    Ok(maybe_event) => match maybe_event {
                        Some(InputEvent::StartBootstrap) => {
                            self.bootstrap().await;
                        }
                        _ => {}
                    },
                    Err(e) => self.bootstrap().await,
                }
            } else if matches!(self.rx.next().await, Some(InputEvent::BootstrapFinished)) {
                self.in_progress = false;
            }
        }
    }
    async fn bootstrap(&mut self) {
        self.in_progress = true;
        self.network_tx.send(ClientRequest::BeginBootstrap).await;
    }
}
