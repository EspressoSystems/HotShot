use async_std::prelude::FutureExt;
use flume::RecvError;
use futures::{future::join_all, Future};
use libp2p::{Multiaddr, PeerId};
use libp2p_networking::network::{
    network_node_handle_error::NodeConfigSnafu, spawn_handler, NetworkEvent,
    NetworkNodeConfigBuilder, NetworkNodeHandle, NetworkNodeHandleError, NetworkNodeType,
};
use phaselock_utils::test_util::{setup_backtrace, setup_logging};
use snafu::{ResultExt, Snafu};
use std::{collections::{HashMap, HashSet}, fmt::Debug, num::NonZeroUsize, sync::Arc, time::Duration};
use tracing::{info, instrument, warn};

/// General function to spin up testing infra
/// perform tests by calling `run_test`
/// then cleans up tests
/// # Panics
/// Panics if unable to:
/// - Initialize logging
/// - Initialize network nodes
/// - Kill network nodes
/// - A test assertion fails
pub async fn test_bed<S: 'static + Send + Default + Debug, F, FutF, G: Clone, FutG>(
    run_test: F,
    client_handler: G,
    num_nodes: usize,
    num_of_bootstrap: usize,
    timeout: Duration,
) where
    FutF: Future<Output = ()>,
    FutG: Future<Output = Result<(), NetworkNodeHandleError>> + 'static + Send + Sync,
    F: FnOnce(Vec<Arc<NetworkNodeHandle<S>>>, Duration) -> FutF,
    G: Fn(NetworkEvent, Arc<NetworkNodeHandle<S>>) -> FutG + 'static + Send + Sync,
{
    setup_logging();
    setup_backtrace();

    // NOTE we want this to panic if we can't spin up the swarms.
    // that amounts to a failed test.
    let handles: Vec<Arc<NetworkNodeHandle<S>>> =
        spin_up_swarms(num_nodes, timeout, num_of_bootstrap)
            .await
            .unwrap();
    for handle in &handles {
        spawn_handler(handle.clone(), client_handler.clone()).await;
    }

    run_test(handles.clone(), timeout).await;

    // cleanup
    for handle in handles {
        handle.shutdown().await.unwrap();
    }
}

fn gen_peerid_map<S>(handles: &[Arc<NetworkNodeHandle<S>>]) -> HashMap<PeerId, usize> {
    let mut r_val = HashMap::new();
    for handle in handles {
        r_val.insert(handle.peer_id(), handle.id());
    }
    r_val
}

/// print the connections for each handle in `handles`
/// useful for debugging
pub async fn print_connections<S>(handles: &[Arc<NetworkNodeHandle<S>>]) {
    let m = gen_peerid_map(handles);
    warn!("PRINTING CONNECTION STATES");
    for handle in handles.iter() {
        warn!(
            "peer {}, connected to {:?}",
            handle.id(),
            handle
                .connected_peers()
                .await
                .iter()
                .map(|pid| m.get(pid).unwrap())
                .collect::<Vec<_>>()
        );
        warn!(
            "peer {}, knowns about {:?}",
            handle.id(),
            handle
                .known_peers()
                .await
                .iter()
                .map(|pid| m.get(pid).unwrap())
                .collect::<Vec<_>>()
        );
    }
}

#[allow(dead_code)]
pub async fn check_connection_state<S>(handles: &[Arc<NetworkNodeHandle<S>>]) {
    let mut err_msg = "".to_string();
    for (i, handle) in handles.iter().enumerate() {
        let state = handle.connection_state().await;
        if state.known_peers.len() < handle.config().min_num_peers
            && handle.config().node_type != NetworkNodeType::Bootstrap
        {
            err_msg.push_str(&format!(
                "\nhad {} known peers for {}-th handle",
                state.known_peers.len(),
                i
            ));
        }
        if state.connected_peers.len() < handle.config().min_num_peers {
            err_msg.push_str(&format!(
                "\nhad {} connected peers for {}-th handle",
                state.connected_peers.len(),
                i
            ));
        }
    }
    if !err_msg.is_empty() {
        panic!("{}", err_msg);
    }
}

/// Spins up `num_of_nodes` nodes, connects them to each other
/// and waits for connections to propagate to all nodes.
#[instrument]
pub async fn spin_up_swarms<S: std::fmt::Debug + Default>(
    num_of_nodes: usize,
    timeout_len: Duration,
    num_bootstrap: usize,
) -> Result<Vec<Arc<NetworkNodeHandle<S>>>, TestError<S>> {
    let mut handles = Vec::new();
    let mut bootstrap_addrs = Vec::<(PeerId, Multiaddr)>::new();
    let mut connecting_futs = Vec::new();
    /// FIXME make this mesh numbers
    let min_num_peers = num_of_nodes / 4;
    let max_num_peers = num_of_nodes / 2;
    // should never panic unless num_nodes is 0
    let replication_factor = NonZeroUsize::new(num_of_nodes).unwrap();

    for i in 0..num_bootstrap {
        let mut config = NetworkNodeConfigBuilder::default();
        config
            .replication_factor(replication_factor)
            .node_type(NetworkNodeType::Bootstrap)
            .max_num_peers(0)
            .min_num_peers(0)
            .to_connect_addrs(bootstrap_addrs.iter().map(|(_, b)| b.clone()).collect())
            ;
        let node = Arc::new(
            NetworkNodeHandle::new(
                config
                    .build()
                    .context(NodeConfigSnafu)
                    .context(HandleSnafu)?,
                i,
            )
            .await
            .context(HandleSnafu)?,
        );
        let addr = node.listen_addr();
        bootstrap_addrs.push((node.peer_id(), addr));
        connecting_futs.push(
            NetworkNodeHandle::wait_to_connect(node.clone(), min_num_peers, node.recv_network(), i)
                .timeout(timeout_len),
        );
        handles.push(node);
    }

    let regular_node_config = NetworkNodeConfigBuilder::default()
        .node_type(NetworkNodeType::Regular)
        .min_num_peers(min_num_peers)
        .max_num_peers(max_num_peers)
        .replication_factor(replication_factor)
        .to_connect_addrs(bootstrap_addrs.iter().map(|(_, b)| b.clone()).collect())
        .build()
        .context(NodeConfigSnafu)
        .context(HandleSnafu)?;

    for j in 0..(num_of_nodes - num_bootstrap) {
        let node = Arc::new(
            // FIXME this should really be a reference
            NetworkNodeHandle::new(regular_node_config.clone(), j + num_bootstrap)
                .await
                .context(HandleSnafu)?,
        );
        let addr = node.listen_addr();
        bootstrap_addrs.push((node.peer_id(), addr));
        connecting_futs.push(
            NetworkNodeHandle::wait_to_connect(
                node.clone(),
                min_num_peers,
                node.recv_network(),
                num_bootstrap + j,
            )
            .timeout(timeout_len),
        );

        handles.push(node);
    }

    info!(
        "known nodes: {:?}",
        bootstrap_addrs
            .iter()
            .map(|(a, b)| (Some(*a), b.clone()))
            .collect::<Vec<_>>()
    );

    for handle in &handles {
        handle
            .add_known_peers(
                bootstrap_addrs
                    .iter()
                    .map(|(a, b)| (Some(*a), b.clone()))
                    .collect::<Vec<_>>(),
            )
            .await
            .context(HandleSnafu)?;
    }

    let res = join_all(connecting_futs.into_iter()).await;
    let mut failing_nodes = Vec::new();
    for (idx, a_node) in res.iter().enumerate() {
        match a_node {
            Ok(Err(_)) | Err(_) => failing_nodes.push(idx),
            Ok(Ok(_)) => (),
        }
    }
    if !failing_nodes.is_empty() {
        return Err(TestError::SpinupTimeout { failing_nodes });
    }

    for handle in &handles {
        handle
            .subscribe("global".to_string())
            .await
            .context(HandleSnafu)?;
    }

    Ok(handles)
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum TestError<S: Debug> {
    #[snafu(display("Channel error {source:?}"))]
    Recv {
        source: RecvError,
    },
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
        expected: S,
        actual: S,
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
