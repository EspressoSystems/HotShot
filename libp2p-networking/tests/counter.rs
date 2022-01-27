use std::sync::Arc;

use async_std::{sync::Mutex, task::spawn};
use bincode::Options;
use flume::{Receiver, RecvError, SendError, Sender};
use futures::{select, Future, FutureExt};
use libp2p::{Multiaddr, PeerId};
use networking_demo::{gen_multiaddr, Network, NetworkError, SwarmAction, SwarmResult};

use serde::{Deserialize, Serialize};

use snafu::{ResultExt, Snafu};
use tracing::instrument;

pub type Counter = u8;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum CounterMessage {
    IncrementCounter {
        from: CounterState,
        to: CounterState,
    },
    AskForCounter,
    MyCounterIs(CounterState),
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default)]
pub struct CounterState(Counter);

#[derive(Debug)]
pub struct SwarmHandle {
    /// the only piece of contested data.
    state: Arc<Mutex<CounterState>>,
    /// send an action to the networkbehaviour
    send_chan: Sender<SwarmAction>,
    /// receive an action from the networkbehaviour
    recv_chan: Receiver<SwarmResult>,
    /// kill the networkbheaviour
    kill_switch: Sender<()>,
    /// receiving end of killing the network behaviour
    recv_kill: Receiver<()>,
    /// the local address we're listening on
    listen_addr: Multiaddr,
    /// the peer id
    peer_id: PeerId,
}

impl SwarmHandle {
    pub async fn new(known_addr: Option<Multiaddr>) -> Result<Self, HandlerError> {
        let listen_addr = gen_multiaddr(0);
        let mut network = Network::new().await.context(NetworkSnafu)?;
        let peer_id = network.peer_id;
        let listen_addr = network
            .start(listen_addr, known_addr)
            .await
            .context(NetworkSnafu)?;
        let (send_chan, recv_chan) = network.spawn_listeners().await.context(NetworkSnafu)?;
        let (kill_switch, recv_kill) = flume::bounded(1);
        Ok(SwarmHandle {
            state: Arc::new(Mutex::new(CounterState::default())),
            send_chan,
            recv_chan,
            kill_switch,
            recv_kill,
            listen_addr,
            peer_id,
        })
    }

    pub async fn kill(&self) -> Result<(), NetworkError> {
        self.send_chan
            .send_async(SwarmAction::Shutdown)
            .await
            .map_err(|_e| NetworkError::StreamClosed)?;
        self.kill_switch
            .send_async(())
            .await
            .map_err(|_e| NetworkError::StreamClosed)?;
        Ok(())
    }
}

pub async fn spin_up_swarms(num_of_nodes: usize) -> Result<Vec<Arc<SwarmHandle>>, HandlerError> {
    let bootstrap = SwarmHandle::new(None).await?;
    let bootstrap_addr = bootstrap.listen_addr.clone();
    let mut handles = Vec::new();
    for _ in 0..(num_of_nodes - 1) {
        handles.push(Arc::new(
            SwarmHandle::new(Some(bootstrap_addr.clone())).await?,
        ));
    }
    Ok(handles)
}

pub async fn handle_event(
    event: SwarmResult,
    handle: Arc<SwarmHandle>,
) -> Result<(), HandlerError> {
    use CounterMessage::*;
    #[allow(clippy::enum_glob_use)]
    use SwarmResult::*;
    let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
    match event {
        GossipMsg(m) | DirectResponse(m) => {
            if let Ok(msg) = bincode_options.deserialize::<'_, CounterMessage>(&m) {
                match msg {
                    MyCounterIs(c) | CounterMessage::IncrementCounter { to: c, .. } => {
                        *handle.state.lock().await = c;
                    }
                    // NOTE doesn't make sense at the request level
                    AskForCounter => {}
                }
            }
        }
        DirectRequest(m, chan) => {
            if let Ok(msg) = bincode_options.deserialize::<'_, CounterMessage>(&m) {
                match msg {
                    IncrementCounter { to, .. } => {
                        *handle.state.lock().await = to;
                    }
                    AskForCounter => {
                        let response = MyCounterIs(handle.state.lock().await.clone());
                        // FIXME error handling
                        let serialized_response = bincode_options
                            .serialize(&response)
                            .context(SerializationSnafu)?;
                        // FIXME error handling
                        handle
                            .send_chan
                            .send_async(SwarmAction::DirectResponse(chan, serialized_response))
                            .await
                            .context(SendSnafu)?
                    }
                    // NOTE doesn't make sense as request type
                    // TODO maybe should check this at the type level
                    CounterMessage::MyCounterIs(_) => {}
                }
            }
        }
        _ => {}
    };
    Ok(())
}

// TODO snafu error handler type that is either a serialization error
// OR channel sending error
// TODO instrumentation
pub async fn spawn_handler<Fut>(
    handle: Arc<SwarmHandle>,
    event_handler: impl (Fn(SwarmResult, Arc<SwarmHandle>) -> Fut)
        + std::marker::Sync
        + std::marker::Send
        + 'static,
) where
    Fut:
        Future<Output = Result<(), HandlerError>> + std::marker::Send + 'static + std::marker::Sync,
{
    let recv_kill = handle.recv_kill.clone();
    let recv_event = handle.recv_chan.clone();
    spawn(async move {
        loop {
            select!(
                _ = recv_kill.recv_async().fuse() => {
                    break;
                },
                event = recv_event.recv_async().fuse() => {
                    event_handler(event.context(RecvSnafu)?, handle.clone()).await?;
                },
            );
        }
        Ok::<(), HandlerError>(())
    });
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
        handle.kill_switch.send_async(()).await.unwrap();
    }
}

#[async_std::test]
#[instrument]
async fn test_request_response() {
    // NOTE we want this to panic if we can't spin up the swarms.
    // that amounts to a failed test.
    let handles = spin_up_swarms(5).await.unwrap();

    // cleanup
    for handle in handles.into_iter() {
        handle.kill().await.unwrap();
    }
}

#[async_std::test]
#[instrument]
async fn test_gossip() {
    // NOTE we want this to panic if we can't spin up the swarms.
    // that amounts to a failed test.
    let handles = spin_up_swarms(5).await.unwrap();
    for handle in handles.iter() {
        spawn_handler(handle.clone(), handle_event).await;
    }

    //

    // cleanup
    for handle in handles.into_iter() {
        handle.kill().await.unwrap();
    }
}

#[derive(Debug, Snafu)]
pub enum HandlerError {
    NetworkError { source: NetworkError },
    SerializationError { source: Box<bincode::ErrorKind> },
    DeserializationError {},
    SendError { source: SendError<SwarmAction> },
    RecvError { source: RecvError },
}
