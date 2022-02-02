use std::sync::{Arc, Once};

use futures::Future;
use networking_demo::{
    network_node::NetworkEvent,
    network_node_handle::{spawn_handler, HandlerError, NetworkNodeHandle},
    tracing_setup,
};
use std::fmt::Debug;
use tracing::warn;

static INIT: Once = Once::new();

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
        tracing_setup::setup_tracing();
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
