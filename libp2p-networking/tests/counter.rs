use std::{sync::Arc, time::Duration};

use async_std::{
    sync::Mutex,
    task::{sleep, spawn},
};
use bincode::Options;
use flume::{Receiver, RecvError, SendError, Sender};
use futures::{select, Future, FutureExt};
use libp2p::{gossipsub::Topic, Multiaddr, PeerId};
use networking_demo::{
    gen_multiaddr, ConnectionData, NetworkError, NetworkNode, NetworkNodeType, SwarmAction,
    SwarmResult,
};
use rand::{seq::IteratorRandom, thread_rng};

use serde::{Deserialize, Serialize};

use snafu::{ResultExt, Snafu};
use std::sync::Once;
use tracing::{error, info, info_span, instrument, warn, Instrument};

pub type Counter = u8;
const TOTAL_NUM_PEERS: usize = 20;

static INIT: Once = Once::new();

/// Message types. We can either
/// - increment the Counter
/// - request a counter value
/// - reply with a counter value
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

/// A handle containing:
/// - A reference to the state
/// - Controls for the swarm
///
#[derive(Debug)]
pub struct SwarmHandle {
    /// the state. TODO make this generic
    state: Arc<Mutex<CounterState>>,
    /// send an action to the networkbehaviour
    send_chan: Sender<SwarmAction>,
    /// receive an action from the networkbehaviour
    recv_chan: Receiver<SwarmResult>,
    /// kill the event handler for events from the swarm
    kill_switch: Sender<()>,
    /// receiving end of `kill_switch`
    recv_kill: Receiver<()>,
    /// the local address we're listening on
    listen_addr: Multiaddr,
    /// the peer id of the networkbehaviour
    peer_id: PeerId,
    /// the connection metadata associated with the networkbehaviour
    connection_state: Arc<Mutex<ConnectionData>>,
}

impl SwarmHandle {
    /// constructs a new node listening on `known_addr`
    #[instrument]
    pub async fn new(
        known_addr: Option<Multiaddr>,
        node_type: NetworkNodeType,
    ) -> Result<Self, HandlerError> {
        //`randomly assigned port
        let listen_addr = gen_multiaddr(0);
        let mut network = NetworkNode::new(node_type).await.context(NetworkSnafu)?;
        let peer_id = network.peer_id;
        let listen_addr = network
            .start(listen_addr, known_addr)
            .await
            .context(NetworkSnafu)?;
        let (send_chan, recv_chan) = network.spawn_listeners().await.context(NetworkSnafu)?;
        let (kill_switch, recv_kill) = flume::bounded(1);

        send_chan
            .send_async(SwarmAction::Subscribe("global".to_string()))
            .await
            .context(SendSnafu)?;

        Ok(SwarmHandle {
            state: Arc::new(Mutex::new(CounterState::default())),
            send_chan,
            recv_chan,
            kill_switch,
            recv_kill,
            listen_addr,
            peer_id,
            connection_state: Default::default(),
        })
    }

    /// Cleanly shuts down a swarm node
    /// This is done by sending a message to
    /// the swarm event handler to stop handling events
    /// and a message to the swarm itself to spin down
    #[instrument]
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

/// spins up `num_of_nodes` nodes and connects them to each other
#[instrument]
pub async fn spin_up_swarms(num_of_nodes: usize) -> Result<Vec<Arc<SwarmHandle>>, HandlerError> {
    // FIXME change API to accomodate multiple bootstrap nodes
    let bootstrap = SwarmHandle::new(None, NetworkNodeType::Bootstrap).await?;
    let bootstrap_addr = bootstrap.listen_addr.clone();
    info!(
        "boostrap node {} on addr {}",
        bootstrap.peer_id, bootstrap_addr
    );
    // give a split second to initialize
    // TODO the proper way to do this is to make it event driven. Once it bootstraps *successfully*
    // THEN add next peer
    sleep(Duration::from_secs(1)).await;
    let mut handles = Vec::new();
    for _ in 0..(num_of_nodes - 1) {
        handles.push(Arc::new(
            SwarmHandle::new(Some(bootstrap_addr.clone()), NetworkNodeType::Regular).await?,
        ));
        sleep(Duration::from_secs(1)).await;
    }
    Ok(handles)
}

/// general function to spin up testing infra
/// perform tests by calling `run_test`
/// then cleans up tests
pub async fn test_bed<F, Fut>(run_test: F)
where
    Fut: Future<Output = ()>,
    F: FnOnce(Vec<Arc<SwarmHandle>>) -> Fut,
{
    // only call once otherwise panics
    // <https://github.com/yaahc/color-eyre/issues/78>
    INIT.call_once(|| {
        color_eyre::install().unwrap();
        networking_demo::tracing_setup::setup_tracing();
    });

    // NOTE we want this to panic if we can't spin up the swarms.
    // that amounts to a failed test.
    let handles: Vec<Arc<SwarmHandle>> = spin_up_swarms(TOTAL_NUM_PEERS).await.unwrap();
    for handle in handles.iter() {
        spawn_handler(handle.clone(), handle_event).await;
    }
    print_connections(&handles).await;

    run_test(handles.clone()).await;

    // cleanup
    for handle in handles.into_iter() {
        handle.kill().await.unwrap();
    }
}

/// event handler for events from the swarm
/// - updates state based on events received
/// - replies to direct messages
#[instrument]
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
            if let Ok(msg) = bincode_options.deserialize::<CounterMessage>(&m) {
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
            if let Ok(msg) = bincode_options.deserialize::<CounterMessage>(&m) {
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
                    MyCounterIs(_) => {}
                }
            }
        }
        UpdateConnectedPeers(p) => {
            handle.connection_state.lock().await.connected_peers = p;
        }
        UpdateKnownPeers(p) => {
            handle.connection_state.lock().await.known_peers = p;
        }
    };
    Ok(())
}

/// Glue function that listens for events from the Swarm corresponding to `handle`
/// and calls `event_handler` when an event is observed.
/// The idea is that this function can be used independent of the actual behaviour
/// we want
#[instrument(skip(event_handler))]
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
    spawn(
        async move {
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
        }
        .instrument(info_span!("Libp2p Counter Handler")),
    );
}

/// given a slice of handles assumed to be larger than 0
/// chooses one
/// # Panics
/// panics if handles is of length 0
pub fn get_random_handle(handles: &[Arc<SwarmHandle>]) -> Arc<SwarmHandle> {
    handles.iter().choose(&mut thread_rng()).unwrap().clone()
}

/// check that we can direct message to increment counter
#[async_std::test]
#[instrument]
async fn test_request_response() {
    async fn run_request_response(handles: Vec<Arc<SwarmHandle>>) {
        let send_handle = get_random_handle(handles.as_slice());
        let recv_handle = get_random_handle(handles.as_slice());

        *send_handle.state.lock().await = CounterState(5);

        let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
        let msg_inner = bincode_options
            .serialize(&CounterMessage::IncrementCounter {
                from: CounterState(0),
                to: CounterState(5),
            })
            .unwrap();
        let msg = SwarmAction::DirectRequest(recv_handle.peer_id, msg_inner);
        send_handle.send_chan.send_async(msg).await.unwrap();

        // block to let the direction message
        // TODO make this event driven
        // e.g. everyone receives the gossipmsg event
        // or timeout
        sleep(Duration::from_millis(10)).await;

        for handle in handles.iter() {
            let expected_state =
                if handle.peer_id == send_handle.peer_id || handle.peer_id == recv_handle.peer_id {
                    CounterState(5)
                } else {
                    CounterState::default()
                };
            assert_eq!(*handle.state.lock().await, expected_state);
        }
    }

    test_bed(run_request_response).await
}

/// check that we can broadcast a message out and get counter increments
#[async_std::test]
#[instrument]
async fn test_gossip() {
    async fn run_gossip(handles: Vec<Arc<SwarmHandle>>) {
        let msg_handle = handles.iter().choose(&mut thread_rng()).unwrap();
        *msg_handle.state.lock().await = CounterState(5);
        let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
        let msg_inner = bincode_options
            .serialize(&CounterMessage::IncrementCounter {
                from: CounterState(0),
                to: CounterState(5),
            })
            .unwrap();
        let (send, recv) = flume::bounded(1);
        let msg = SwarmAction::GossipMsg(Topic::new("global"), msg_inner, send);
        msg_handle.send_chan.send_async(msg).await.unwrap();
        recv.recv_async().await.unwrap().unwrap();

        // block to let the gossipping happen
        // TODO make this event driven
        // e.g. everyone receives the gossipmsg event
        // or timeout
        sleep(Duration::from_millis(10)).await;

        let mut failing_idxs = Vec::new();
        for (i, handle) in handles.iter().enumerate() {
            if *handle.state.lock().await != CounterState(5) {
                failing_idxs.push(i);
            }
        }
        if !failing_idxs.is_empty() {
            error!(?failing_idxs, "failing idxs!!");
            panic!("some nodes did not receive the message {:?}", failing_idxs);
        }
    }

    test_bed(run_gossip).await;
}

/// print the connections for each handle in `handles`
/// useful for debugging
async fn print_connections(handles: &[Arc<SwarmHandle>]) {
    warn!("PRINTING CONNECTION STATES");
    for (i, handle) in handles.iter().enumerate() {
        warn!(
            "peer {}, connected to {:?}",
            i,
            handle.connection_state.lock().await.connected_peers
        );
        warn!(
            "peer {}, knowns about {:?}",
            i,
            handle.connection_state.lock().await.known_peers
        );
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
