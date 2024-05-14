use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    num::NonZeroUsize,
    str::FromStr,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use futures::{future::join_all, Future, FutureExt};
use libp2p::{identity::Keypair, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_networking::network::{
    network_node_handle_error::NodeConfigSnafu, spawn_network_node, NetworkEvent,
    NetworkNodeConfigBuilder, NetworkNodeHandle, NetworkNodeHandleError, NetworkNodeReceiver,
    NetworkNodeType,
};
use snafu::{ResultExt, Snafu};
use tokio::{
    select, spawn,
    sync::{broadcast, Notify},
    time::{sleep, timeout},
};
use tracing::{info, instrument, warn};

#[derive(Clone, Debug)]
pub(crate) struct HandleWithState {
    pub(crate) handle: Arc<NetworkNodeHandle>,
    state: Arc<AtomicU32>,
    state_change: Arc<Notify>,
}

impl HandleWithState {
    pub fn load_state(&self) -> u32 {
        self.state.load(Ordering::SeqCst)
    }

    pub fn increment_state(&self) {
        self.state.fetch_add(1, Ordering::SeqCst);
        self.state_change.notify_waiters();
    }

    pub fn set_state(&self, new_state: u32) {
        self.state.store(new_state, Ordering::SeqCst);
        self.state_change.notify_waiters();
    }

    pub fn compare_exchange_state(&self, current: u32, new: u32) -> bool {
        self.state
            .compare_exchange(current, new, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub async fn wait_for_state(&self, timeout_duration: Duration) -> bool {
        timeout(timeout_duration, self.state_change.notified())
            .await
            .is_ok()
    }
}

/// Spawn a handler `F` that will be notified every time a new [`NetworkEvent`] arrives.
///
/// # Panics
///
/// Will panic if a handler is already spawned
pub fn spawn_handler<F, RET>(
    handle_and_state: HandleWithState,
    mut receiver: NetworkNodeReceiver,
    cb: F,
) -> impl Future
where
    F: Fn(NetworkEvent, HandleWithState) -> RET + Sync + Send + 'static,
    RET: Future<Output = Result<(), NetworkNodeHandleError>> + Send + 'static,
{
    spawn(async move {
        let Some(mut kill_switch) = receiver.take_kill_switch() else {
            tracing::error!(
                "`spawn_handle` was called on a network handle that was already closed"
            );
            return;
        };

        loop {
            select! {
                incoming_message = receiver.recv() => {
                    let incoming_message = match incoming_message {
                        Ok(msg) => msg,
                        Err(e) => {
                            tracing::warn!(?e, "NetworkNodeHandle::spawn_handle was unable to receive more messages");
                            return;
                        }
                    };
                    if let Err(e) = cb(incoming_message, handle_and_state.clone()).await {
                        tracing::error!(?e, "NetworkNodeHandle::spawn_handle returned an error");
                        return;
                    }
                }
                _ = kill_switch.recv() => {
                    return;
                }
            }
        }
    })
}

/// General function to spin up testing infra
/// perform tests by calling `run_test`
/// then cleans up tests
/// # Panics
/// Panics if unable to:
/// - Initialize logging
/// - Initialize network nodes
/// - Kill network nodes
/// - A test assertion fails
pub async fn test_bed<F, FutF, G, FutG>(
    run_test: F,
    client_handler: G,
    num_nodes: usize,
    num_of_bootstrap: usize,
    timeout: Duration,
) where
    FutF: Future<Output = ()>,
    FutG: Future<Output = Result<(), NetworkNodeHandleError>> + 'static + Send + Sync,
    F: FnOnce(Vec<HandleWithState>, Duration) -> FutF,
    G: Fn(NetworkEvent, HandleWithState) -> FutG + 'static + Send + Sync + Clone,
{
    let mut kill_switches = Vec::new();
    // NOTE we want this to panic if we can't spin up the swarms.
    // that amounts to a failed test.
    let handles_and_receivers = spin_up_swarms(num_nodes, timeout, num_of_bootstrap)
        .await
        .unwrap();

    let (handles, receivers): (Vec<_>, Vec<_>) = handles_and_receivers.into_iter().unzip();
    let mut handler_futures = Vec::new();
    for (i, mut rx) in receivers.into_iter().enumerate() {
        let (kill_tx, kill_rx) = broadcast::channel(1);
        let handle = &handles[i];
        kill_switches.push(kill_tx);
        rx.set_kill_switch(kill_rx);
        let handler_fut = spawn_handler(handle.clone(), rx, client_handler.clone());
        handler_futures.push(handler_fut);
    }

    run_test(handles.clone(), timeout).await;

    // cleanup
    for handle in handles {
        handle.handle.shutdown().await.unwrap();
    }
    for switch in kill_switches {
        let _ = switch.send(());
    }

    for fut in handler_futures {
        fut.await;
    }
}

fn gen_peerid_map(handles: &[Arc<NetworkNodeHandle>]) -> HashMap<PeerId, usize> {
    let mut r_val = HashMap::new();
    for handle in handles {
        r_val.insert(handle.peer_id(), handle.id());
    }
    r_val
}

/// print the connections for each handle in `handles`
/// useful for debugging
pub async fn print_connections(handles: &[Arc<NetworkNodeHandle>]) {
    let m = gen_peerid_map(handles);
    warn!("PRINTING CONNECTION STATES");
    for handle in handles {
        warn!(
            "peer {}, connected to {:?}",
            handle.id(),
            handle
                .connected_pids()
                .await
                .unwrap()
                .iter()
                .map(|pid| m.get(pid).unwrap())
                .collect::<Vec<_>>()
        );
    }
}

/// Spins up `num_of_nodes` nodes, connects them to each other
/// and waits for connections to propagate to all nodes.
#[allow(clippy::type_complexity)]
#[instrument]
pub async fn spin_up_swarms(
    num_of_nodes: usize,
    timeout_len: Duration,
    num_bootstrap: usize,
) -> Result<Vec<(HandleWithState, NetworkNodeReceiver)>, TestError> {
    let mut handles = Vec::new();
    let mut bootstrap_addrs = Vec::<(PeerId, Multiaddr)>::new();
    let mut connecting_futs = Vec::new();
    // should never panic unless num_nodes is 0
    let replication_factor = NonZeroUsize::new(num_of_nodes - 1).unwrap();

    for i in 0..num_bootstrap {
        let mut config = NetworkNodeConfigBuilder::default();
        let identity = Keypair::generate_ed25519();
        // let start_port = 5000;
        // NOTE use this if testing locally and want human readable ports
        // as opposed to random ports. These are harder to track
        // especially since the "listener"/inbound connection sees a different
        // port
        // let addr = Multiaddr::from_str(&format!("/ip4/127.0.0.1/udp/{}/quic-v1", start_port + i)).unwrap();

        let addr = Multiaddr::from_str("/ip4/127.0.0.1/udp/0/quic-v1").unwrap();
        config
            .identity(identity)
            .replication_factor(replication_factor)
            .node_type(NetworkNodeType::Bootstrap)
            .to_connect_addrs(HashSet::default())
            .bound_addr(Some(addr))
            .ttl(None)
            .republication_interval(None)
            .server_mode(true);
        let config = config
            .build()
            .context(NodeConfigSnafu)
            .context(HandleSnafu)?;
        let (rx, node) = spawn_network_node(config.clone(), i).await.unwrap();
        let node = Arc::new(node);
        let addr = node.listen_addr();
        info!("listen addr for {} is {:?}", i, addr);
        bootstrap_addrs.push((node.peer_id(), addr));
        connecting_futs.push({
            let node = Arc::clone(&node);
            async move {
                node.begin_bootstrap()?;
                node.lookup_pid(PeerId::random()).await
            }
            .boxed_local()
        });
        let node_with_state = HandleWithState {
            handle: Arc::clone(&node),
            state: Arc::default(),
            state_change: Arc::default(),
        };
        handles.push((node_with_state, rx));
    }

    for j in 0..(num_of_nodes - num_bootstrap) {
        let addr = Multiaddr::from_str("/ip4/127.0.0.1/udp/0/quic-v1").unwrap();
        // NOTE use this if testing locally and want human readable ports
        // let addr = Multiaddr::from_str(&format!(
        //     "/ip4/127.0.0.1/udp/{}/quic-v1",
        //     start_port + num_bootstrap + j
        // )).unwrap();
        let regular_node_config = NetworkNodeConfigBuilder::default()
            .node_type(NetworkNodeType::Regular)
            .replication_factor(replication_factor)
            .bound_addr(Some(addr.clone()))
            .to_connect_addrs(HashSet::default())
            .server_mode(true)
            .build()
            .context(NodeConfigSnafu)
            .context(HandleSnafu)?;
        let (rx, node) = spawn_network_node(regular_node_config.clone(), j + num_bootstrap)
            .await
            .unwrap();

        let node = Arc::new(node);
        connecting_futs.push({
            let node = Arc::clone(&node);
            async move {
                node.begin_bootstrap()?;
                node.lookup_pid(PeerId::random()).await
            }
            .boxed_local()
        });
        let node_with_state = HandleWithState {
            handle: Arc::clone(&node),
            state: Arc::default(),
            state_change: Arc::default(),
        };
        handles.push((node_with_state, rx));
    }
    info!("BSADDRS ARE: {:?}", bootstrap_addrs);

    info!(
        "known nodes: {:?}",
        bootstrap_addrs
            .iter()
            .map(|(a, b)| (Some(*a), b.clone()))
            .collect::<Vec<_>>()
    );

    for (handle, _) in &handles[0..num_of_nodes] {
        let to_share = bootstrap_addrs.clone();
        handle
            .handle
            .add_known_peers(to_share)
            .context(HandleSnafu)?;
    }

    let res = join_all(connecting_futs.into_iter()).await;
    let mut failing_nodes = Vec::new();
    for (idx, a_node) in res.iter().enumerate() {
        if a_node.is_err() {
            failing_nodes.push(idx);
        }
    }
    if !failing_nodes.is_empty() {
        return Err(TestError::SpinupTimeout { failing_nodes });
    }

    for (handle, _) in &handles {
        handle
            .handle
            .subscribe("global".to_string())
            .await
            .context(HandleSnafu)?;
    }

    sleep(Duration::from_secs(5)).await;

    Ok(handles)
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum TestError {
    #[snafu(display("Channel error: recv failed"))]
    Recv,
    #[snafu(display(
        "Timeout while running direct message round. Timed out when {requester} dmed {requestee}"
    ))]
    DirectTimeout {
        requester: usize,
        requestee: usize,
    },
    #[snafu(display("Timeout while running gossip round. Timed out on {failing:?}."))]
    GossipTimeout {
        failing: Vec<usize>,
    },
    #[snafu(display(
        "Inconsistent state while running test. Expected {expected:?}, got {actual:?} on node {id}"
    ))]
    State {
        id: usize,
        expected: u32,
        actual: u32,
    },
    #[snafu(display("Handler error while running test. {source:?}"))]
    Handle {
        source: NetworkNodeHandleError,
    },
    #[snafu(display("Failed to spin up nodes. Hit timeout instead. {failing_nodes:?}"))]
    SpinupTimeout {
        failing_nodes: Vec<usize>,
    },
    DHTTimeout,
}
