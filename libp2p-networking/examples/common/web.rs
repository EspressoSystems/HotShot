use flume::bounded;
use networking_demo::network_node_handle::NetworkNodeHandle;
use std::{net::SocketAddr, sync::Arc};
use tracing::{debug, error, info};

/// Spawn a web server on the given `addr`.
/// This web server will host the static HTML page `/web/index.html` and expose a `sse` endpoint.
/// This `sse` endpoint will send status updates to the connected clients whenever `NetworkNodeHandle::state_changed` triggers.
///
/// # Links
/// - SSE on wikipedia: <https://en.wikipedia.org/wiki/Server-sent_events>
/// - SEE in `tide`: <https://docs.rs/tide/0.16.0/tide/sse/index.html>
pub fn spawn_server<S>(state: Arc<NetworkNodeHandle<S>>, addr: SocketAddr)
where
    S: WebInfo + Send + 'static,
{
    let mut tide = tide::with_state(state);
    // Unwrap this in the calling thread so that if it fails we fail completely
    // instead of not knowing why the web UI does not work
    tide.at("/")
        .serve_file("web/index.html")
        .expect("Could not register web/index.html");
    tide.at("/sse").get(tide::sse::endpoint(
        |req: tide::Request<Arc<NetworkNodeHandle<S>>>, sender| async move {
            let peer_addr = req.peer_addr();
            debug!(?peer_addr, "Web client connected, sending initial state");

            let state = Arc::clone(req.state());
            network_state::State::new(&state)
                .await
                .send(&sender)
                .await?;

            // Register a `Sender<()>` with the `NetworkNodeHandle` so we get notified when it changes
            let receiver = {
                let (sender, receiver) = bounded(100);
                let mut lock = state.webui_listeners.lock().await;
                lock.push(sender);
                receiver
            };

            while let Ok(()) = receiver.recv_async().await {
                // TODO: I think this will not work as this `.lock` will conflict with the other lock, but we'll see
                if let Err(e) = network_state::State::new(&state).await.send(&sender).await {
                    debug!(?peer_addr, ?e, "Could not send to client, aborting");
                    break;
                }
            }
            Ok(())
        },
    ));
    async_std::task::spawn(async move {
        info!(?addr, "Web UI listening on");
        if let Err(e) = tide.listen(addr).await {
            error!(?e, "Web UI crashed, this is a bug");
        }
    });
}

mod network_state {
    use std::collections::HashSet;

    use libp2p::PeerId;
    use networking_demo::{
        network_node::{ConnectionData, NetworkNodeConfig},
        network_node_handle::NetworkNodeHandle,
    };

    #[derive(serde::Serialize)]
    pub struct State<S: serde::Serialize> {
        pub network_config: NetworkConfig,
        pub state: S,
        pub connection_state: ConnectionState,
    }

    #[derive(serde::Serialize)]
    pub struct NetworkConfig {
        pub max_num_peers: usize,
        pub min_num_peers: usize,
        pub node_type: String,
        pub identity: Option<String>,
        pub ignored_peers: Vec<String>,
    }

    #[derive(serde::Serialize)]
    pub struct ConnectionState {
        pub connected_peers: Vec<String>,
        pub connecting_peers: Vec<String>,
        pub known_peers: Vec<String>,
    }

    impl<S: serde::Serialize> State<S> {
        pub async fn new<W>(handle: &NetworkNodeHandle<W>) -> Self
        where
            W: super::WebInfo<Serialized = S> + Send + 'static,
        {
            Self {
                network_config: NetworkConfig::new(&handle.network_config),
                state: handle.state.lock().await.get_serializable(),
                connection_state: ConnectionState::new(&*handle.connection_state.lock().await),
            }
        }
        pub async fn send(self, sender: &tide::sse::Sender) -> std::io::Result<()> {
            let str = serde_json::to_string(&self).unwrap(); // serializing JSON should never fail
            sender.send("node_state", &str, None).await
        }
    }
    impl NetworkConfig {
        fn new(c: &NetworkNodeConfig) -> Self {
            Self {
                max_num_peers: c.max_num_peers,
                min_num_peers: c.min_num_peers,
                node_type: format!("{:?}", c.node_type),
                identity: c
                    .identity
                    .as_ref()
                    .map(|i| i.public().to_peer_id().to_string()),
                ignored_peers: c.ignored_peers.iter().map(|p| p.to_string()).collect(),
            }
        }
    }
    impl ConnectionState {
        fn new(lock: &ConnectionData) -> ConnectionState {
            fn map(set: &HashSet<PeerId>) -> Vec<String> {
                set.iter().map(|p| p.to_string()).collect()
            }
            Self {
                connected_peers: map(&lock.connected_peers),
                connecting_peers: map(&lock.connecting_peers),
                known_peers: map(&lock.known_peers),
            }
        }
    }
}

/// Trait to unify the info that can be send to the web interface.
///
/// This has to be implemented for all `S` in `NetworkNodeHandle<S>`, e.g. `CounterState`, `ConductorState`, etc.
pub trait WebInfo: Sync + Send {
    type Serialized: serde::Serialize + Send;

    fn get_serializable(&self) -> Self::Serialized;
}
