use flume::{Receiver, SendError, Sender};
use libp2p::Multiaddr;
use networking_demo::{gen_multiaddr, Network, NetworkError, SwarmAction, SwarmResult};

use serde::{Deserialize, Serialize};
use tracing::instrument;

pub type Counter = u8;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum CounterMessage {
    IncrementCounter { from: Counter, to: Counter },
    AskForCounter,
    MyCounterIs(Counter),
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct CounterState(Counter);

#[derive(Clone, Debug)]
pub struct SwarmHandle {
    counter: CounterState,
    send_chan: Sender<SwarmAction>,
    recv_chan: Receiver<SwarmResult>,
    listen_addr: Multiaddr,
}

impl SwarmHandle {
    pub async fn new(known_addr: Option<Multiaddr>) -> Result<Self, NetworkError> {
        let listen_addr = gen_multiaddr(0);
        let mut network = Network::new().await?;
        let listen_addr = network.start(listen_addr, known_addr).await?;
        let (send_chan, recv_chan) = network.spawn_listeners().await?;
        Ok(SwarmHandle {
            counter: CounterState::default(),
            send_chan,
            recv_chan,
            listen_addr,
        })
    }

    pub async fn shutdown(&self) -> Result<(), SendError<SwarmAction>> {
        self.send_chan.send_async(SwarmAction::Shutdown).await
    }
}

impl Default for CounterState {
    fn default() -> Self {
        Self(0)
    }
}

pub async fn spin_up_swarms(num_of_nodes: usize) -> Result<Vec<SwarmHandle>, NetworkError> {
    let bootstrap = SwarmHandle::new(None).await?;
    let bootstrap_addr = bootstrap.listen_addr.clone();
    let mut handles = Vec::new();
    for _ in 0..(num_of_nodes - 1) {
        handles.push(SwarmHandle::new(Some(bootstrap_addr.clone())).await?);
    }
    Ok(handles)
}

#[async_std::test]
#[instrument]
async fn test_spinup() {
    // NOTE we want this to panic if we can't spin up the swarms.
    // that amounts to a failed test.
    let handles = spin_up_swarms(5).await.unwrap();
    for handle in handles.iter() {
        handle
            .send_chan
            .send_async(SwarmAction::Shutdown)
            .await
            .unwrap();
    }
}

#[async_std::test]
#[instrument]
async fn test_counter() {
    // NOTE we want this to panic if we can't spin up the swarms.
    // that amounts to a failed test.
    let handles = spin_up_swarms(5).await.unwrap();
    for handle in handles.iter() {
        handle
            .send_chan
            .send_async(SwarmAction::Shutdown)
            .await
            .unwrap();
    }
}
