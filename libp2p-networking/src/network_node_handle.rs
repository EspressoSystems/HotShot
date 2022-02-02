use async_std::{
    sync::Mutex,
    task::{sleep, spawn},
};

use crate::network_node::{
    gen_multiaddr, ClientRequest, ConnectionData, NetworkError, NetworkEvent, NetworkNode,
    NetworkNodeType,
};
use flume::{Receiver, RecvError, SendError, Sender};
use futures::{select, Future, FutureExt, future::join};
use libp2p::{Multiaddr, PeerId};
use rand::{seq::IteratorRandom, thread_rng};
use snafu::{ResultExt, Snafu};
use std::{
    fmt::Debug,
    sync::{Arc, Once},
    time::Duration,
};
use tracing::{info, info_span, instrument, warn, Instrument};

static INIT: Once = Once::new();

/// A handle containing:
/// - A reference to the state
/// - Controls for the swarm
#[derive(Debug)]
pub struct NetworkNodeHandle<S> {
    /// the state of the replica
    pub state: Arc<Mutex<S>>,
    /// send an action to the networkbehaviour
    pub send_network: Sender<ClientRequest>,
    /// receive an action from the networkbehaviour
    pub recv_network: Receiver<NetworkEvent>,
    /// kill the event handler for events from the swarm
    pub kill_switch: Sender<()>,
    /// receiving end of `kill_switch`
    pub recv_kill: Receiver<()>,
    /// the local address we're listening on
    pub listen_addr: Multiaddr,
    /// the peer id of the networkbehaviour
    pub peer_id: PeerId,
    /// the connection metadata associated with the networkbehaviour
    pub connection_state: Arc<Mutex<ConnectionData>>,
}

impl<S: Default + Debug> NetworkNodeHandle<S> {
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
            .send_async(ClientRequest::Subscribe("global".to_string()))
            .await
            .context(SendSnafu)?;

        Ok(NetworkNodeHandle {
            state: Arc::new(Mutex::new(S::default())),
            send_network: send_chan,
            recv_network: recv_chan,
            kill_switch,
            recv_kill,
            listen_addr,
            peer_id,
            connection_state: Arc::default(),
        })
    }

    /// Cleanly shuts down a swarm node
    /// This is done by sending a message to
    /// the swarm event handler to stop handling events
    /// and a message to the swarm itself to spin down
    #[instrument]
    pub async fn kill(&self) -> Result<(), NetworkError> {
        self.send_network
            .send_async(ClientRequest::Shutdown)
            .await
            .map_err(|_e| NetworkError::StreamClosed)?;
        self.kill_switch
            .send_async(())
            .await
            .map_err(|_e| NetworkError::StreamClosed)?;
        Ok(())
    }

    /// spins up `num_of_nodes` nodes and connects them to each other
    #[instrument]
    pub async fn spin_up_swarms(num_of_nodes: usize) -> Result<Vec<Arc<Self>>, HandlerError> {
        use NetworkEvent::*;
        // FIXME change API to accomodate multiple bootstrap nodes
        let bootstrap: NetworkNodeHandle<S> =
            NetworkNodeHandle::new(None, NetworkNodeType::Bootstrap).await?;
        let bootstrap_addr = bootstrap.listen_addr.clone();
        info!(
            "boostrap node {} on addr {}",
            bootstrap.peer_id, bootstrap_addr
        );
        let mut handles = Vec::new();
        println!("bootstrap addr is: {:?}", bootstrap_addr);

        async fn wait_to_connect(num_of_nodes: usize, chan: Receiver<NetworkEvent>, node_idx: usize){
            'a: loop {
                if let NetworkEvent::UpdateConnectedPeers(pids) = chan.recv_async().await.context(RecvSnafu).unwrap(){
                    if pids.len() >= 15 {
                        println!("node {} done", node_idx);
                        break 'a;
                    }
                    else {
                        println!("node {} connected to {:?} nodes", node_idx, pids.len());
                    }
                } 
            }
        }


        let mut connecting_futs = vec![wait_to_connect(num_of_nodes, bootstrap.recv_network.clone(), 0)];
        for i in 0..(num_of_nodes - 1) {
            let node = Arc::new(NetworkNodeHandle::new(Some(bootstrap_addr.clone()), NetworkNodeType::Regular).await?);
            connecting_futs.push(wait_to_connect(num_of_nodes, node.recv_network.clone(), i+1));

            handles.push(node);
        }
        futures::future::join_all(connecting_futs.into_iter()).await;
        Ok(handles)
    }
}

/// General function to spin up testing infra
/// perform tests by calling `run_test`
/// then cleans up tests
/// # Panics
/// Panics if unable to:
/// - Initialize logging
/// - Initialize network nodes
/// - Kill network nodes
/// - a test assertion fails
pub async fn test_bed<S: 'static + Send + Default + Debug, F, FutF, G: Clone, FutG>(
    run_test: F,
    client_handler: G,
    num_nodes: usize,
) where
    FutF: Future<Output = ()>,
    FutG: Future<Output = Result<(), HandlerError>> + 'static + Send + Sync,
    F: FnOnce(Vec<Arc<NetworkNodeHandle<S>>>) -> FutF,
    G: Fn(NetworkEvent, Arc<NetworkNodeHandle<S>>) -> FutG + 'static + Send + Sync,
{
    // only call once otherwise panics
    // <https://github.com/yaahc/color-eyre/issues/78>
    INIT.call_once(|| {
        color_eyre::install().unwrap();
        crate::tracing_setup::setup_tracing();
    });

    // NOTE we want this to panic if we can't spin up the swarms.
    // that amounts to a failed test.
    let handles: Vec<Arc<NetworkNodeHandle<S>>> =
        NetworkNodeHandle::spin_up_swarms(num_nodes).await.unwrap();
    for handle in &handles {
        spawn_handler(handle.clone(), client_handler.clone()).await;
    }
    print_connections(&handles).await;

    run_test(handles.clone()).await;

    // cleanup
    for handle in handles {
        handle.kill().await.unwrap();
    }
}

/// Glue function that listens for events from the Swarm corresponding to `handle`
/// and calls `event_handler` when an event is observed.
/// The idea is that this function can be used independent of the actual behaviour
/// we want
#[allow(clippy::panic)]
#[instrument(skip(event_handler))]
pub async fn spawn_handler<S: 'static + Send + Default + Debug, Fut>(
    handle: Arc<NetworkNodeHandle<S>>,
    event_handler: impl (Fn(NetworkEvent, Arc<NetworkNodeHandle<S>>) -> Fut)
        + std::marker::Sync
        + std::marker::Send
        + 'static,
) where
    Fut:
        Future<Output = Result<(), HandlerError>> + std::marker::Send + 'static + std::marker::Sync,
{
    let recv_kill = handle.recv_kill.clone();
    let recv_event = handle.recv_network.clone();
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
pub fn get_random_handle<S>(handles: &[Arc<NetworkNodeHandle<S>>]) -> Arc<NetworkNodeHandle<S>> {
    handles.iter().choose(&mut thread_rng()).unwrap().clone()
}

/// print the connections for each handle in `handles`
/// useful for debugging
async fn print_connections<S>(handles: &[Arc<NetworkNodeHandle<S>>]) {
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

/// error wrapper type for interacting with swarm handle
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum HandlerError {
    /// error generating network
    NetworkError {
        /// source of error
        source: NetworkError,
    },
    /// failure to serialize a message
    SerializationError {
        /// source of error
        source: Box<bincode::ErrorKind>,
    },
    /// failure to deserialize a message
    DeserializationError {},
    /// error sending request to network
    SendError {
        /// source of error
        source: SendError<ClientRequest>,
    },
    /// error receiving message from network
    RecvError {
        /// source of error
        source: RecvError,
    },
}
