use async_std::{prelude::FutureExt, task::sleep};
use flume::RecvError;
use futures::{future::join_all, Future};
use libp2p::{identity::Keypair, Multiaddr, PeerId};
use libp2p_networking::network::{
    network_node_handle_error::NodeConfigSnafu, spawn_handler, NetworkEvent,
    NetworkNodeConfigBuilder, NetworkNodeHandle, NetworkNodeHandleError, NetworkNodeType,
};
use phaselock_utils::test_util::{setup_backtrace, setup_logging};
use snafu::{ResultExt, Snafu};
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    num::NonZeroUsize,
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use tracing::{error, info, instrument, warn};

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
                .connected_pids()
                .await
                .unwrap()
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
    // should never panic unless num_nodes is 0
    let replication_factor = NonZeroUsize::new(num_of_nodes).unwrap();

    for i in 0..num_bootstrap {
        let mut config = NetworkNodeConfigBuilder::default();
        let identity = Keypair::generate_ed25519();
        // let start_port = 5000;
        // NOTE use this if testing locally and want human readable ports
        // as opposed to random ports. These are harder to track
        // especially since the "listener"/inbound connection sees a different
        // port
        // let addr = Multiaddr::from_str(&format!("/ip4/127.0.0.1/tcp/", start_port + i)).unwrap();

        let addr = Multiaddr::from_str(&format!("/ip4/127.0.0.1/tcp/0")).unwrap();
        config
            .identity(identity)
            .replication_factor(replication_factor)
            .node_type(NetworkNodeType::Bootstrap)
            .bound_addr(Some(addr))
            .to_connect_addrs(HashSet::new());
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
        error!("listen addr for {} is {:?}", i, addr);
        bootstrap_addrs.push((node.peer_id(), addr));
        connecting_futs.push(
            NetworkNodeHandle::wait_to_connect(node.clone(), 4, node.recv_network(), i)
                .timeout(timeout_len),
        );
        handles.push(node);
    }

    for j in 0..(num_of_nodes - num_bootstrap) {
        let addr = Multiaddr::from_str(&format!("/ip4/127.0.0.1/tcp/0")).unwrap();
        // NOTE use this if testing locally and want human readable ports
        // let addr = Multiaddr::from_str(&format!(
        //     "/ip4/127.0.0.1/tcp/{}",
        //     start_port + num_bootstrap + j
        // )).unwrap();
        let regular_node_config = NetworkNodeConfigBuilder::default()
            .node_type(NetworkNodeType::Regular)
            .replication_factor(replication_factor)
            .bound_addr(Some(addr.clone()))
            .to_connect_addrs(HashSet::default())
            .build()
            .context(NodeConfigSnafu)
            .context(HandleSnafu)?;
        let node = Arc::new(
            NetworkNodeHandle::new(regular_node_config.clone(), j + num_bootstrap)
                .await
                .context(HandleSnafu)?,
        );
        connecting_futs.push(
            NetworkNodeHandle::wait_to_connect(
                node.clone(),
                // connected to 4 nodes to be "ready"
                4,
                node.recv_network(),
                num_bootstrap + j,
            )
            .timeout(timeout_len),
        );

        handles.push(node);
    }
    error!("BSADDRS ARE: {:?}", bootstrap_addrs);

    info!(
        "known nodes: {:?}",
        bootstrap_addrs
            .iter()
            .map(|(a, b)| (Some(*a), b.clone()))
            .collect::<Vec<_>>()
    );

    for (idx, handle) in handles[0..num_of_nodes].iter().enumerate() {
        let to_share = if idx < num_bootstrap {
            bootstrap_addrs.clone()
        } else {
            bootstrap_addrs.clone()
        };
        handle
            .add_known_peers(
                to_share
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

    sleep(Duration::from_secs(5)).await;

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
