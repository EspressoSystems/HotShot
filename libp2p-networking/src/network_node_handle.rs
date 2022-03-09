use crate::{
    network::{
        network_node_handle_error::{RecvSnafu, TimeoutSnafu},
        NetworkNodeConfig, NetworkNodeHandle, NetworkNodeHandleError,
    },
    network_node::{ClientRequest, NetworkEvent},
};
use async_std::{future::timeout, task::spawn};
use futures::{select, Future, FutureExt};
use libp2p::{Multiaddr, PeerId};
use rand::{seq::IteratorRandom, thread_rng};
use snafu::ResultExt;
use std::{fmt::Debug, sync::Arc, time::Duration};
use tracing::{info, info_span, instrument, Instrument};

/// Glue function that listens for events from the Swarm corresponding to `handle`
/// and calls `event_handler` when an event is observed.
/// The idea is that this function can be used independent of the actual state
/// we use
#[allow(clippy::panic)]
#[instrument(skip(event_handler))]
pub async fn spawn_handler<S: 'static + Send + Default + Debug, Fut>(
    handle: Arc<NetworkNodeHandle<S>>,
    event_handler: impl (Fn(NetworkEvent, Arc<NetworkNodeHandle<S>>) -> Fut)
        + std::marker::Sync
        + std::marker::Send
        + 'static,
) where
    Fut: Future<Output = Result<(), NetworkNodeHandleError>>
        + std::marker::Send
        + 'static
        + std::marker::Sync,
{
    let recv_kill = handle.recv_kill();
    let recv_event = handle.recv_network();
    spawn(
        async move {
            loop {
                select!(
                    _ = recv_kill.recv_async().fuse() => {
                        handle.mark_killed().await;
                        break;
                    },
                    event = recv_event.recv_async().fuse() => {
                        event_handler(event.context(RecvSnafu)?, handle.clone()).await?;
                    },
                );
            }
            Ok::<(), NetworkNodeHandleError>(())
        }
        .instrument(info_span!("Libp2p Counter Handler")),
    );
}

/// a single node, connects them to each other
/// and waits for connections to propagate to all nodes.
#[instrument]
pub async fn spin_up_swarm<S: std::fmt::Debug + Default>(
    timeout_len: Duration,
    known_nodes: Vec<(Option<PeerId>, Multiaddr)>,
    config: NetworkNodeConfig,
    idx: usize,
    handle: &Arc<NetworkNodeHandle<S>>,
) -> Result<(), NetworkNodeHandleError> {
    info!("known_nodes{:?}", known_nodes);
    handle
        .send_request(ClientRequest::AddKnownPeers(known_nodes))
        .await?;

    timeout(
        timeout_len,
        NetworkNodeHandle::wait_to_connect(
            handle.clone(),
            config.max_num_peers,
            handle.recv_network(),
            idx,
        ),
    )
    .await
    .context(TimeoutSnafu)??;
    handle
        .send_request(ClientRequest::Subscribe("global".to_string()))
        .await?;

    Ok(())
}

/// Given a slice of handles assumed to be larger than 0,
/// chooses one
/// # Panics
/// panics if handles is of length 0
pub fn get_random_handle<S>(handles: &[Arc<NetworkNodeHandle<S>>]) -> Arc<NetworkNodeHandle<S>> {
    handles.iter().choose(&mut thread_rng()).unwrap().clone()
}
