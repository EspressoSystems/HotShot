use std::{
    collections::HashMap,
    sync::{Arc, Once},
    time::Duration,
};

use bincode::Options;
use libp2p::{
    build_multiaddr, gossipsub::Topic, request_response::ResponseChannel, Multiaddr, PeerId,
};
use networking_demo::{
    direct_message::DirectMessageResponse,
    network_node::{ClientRequest, NetworkEvent, NetworkNodeConfigBuilder, NetworkNodeType},
    network_node_handle::{
        spawn_handler, spin_up_swarm, NetworkNodeHandle, NetworkNodeHandleError, NetworkSnafu,
        NodeConfigSnafu, SendSnafu, SerializationSnafu,
    },
    tracing_setup,
};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::fmt::Debug;
use structopt::StructOpt;
use tracing::instrument;

pub type CounterState = u32;
pub type ControllerState = HashMap<PeerId, CounterState>;

/// Normal message types. We can either
/// - increment the Counter
/// - request a counter value
/// - reply with a counter value
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum CounterRequest {
    IncrementCounter {
        from: CounterState,
        to: CounterState,
    },
    AskForCounter,
    MyCounterIs(CounterState),
    Kill,
    Recvd,
}

/// overall message
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum Message {
    /// message to send from a peer to a peer
    NormalMessage(CounterRequest),
    /// message a conductor sent to a node
    /// that the node must send to other node(s)
    ConductorMessage(CounterRequest, ConductorMessageMethod),
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum ConductorMessageMethod {
    Broadcast,
    DirectMessage(PeerId),
}

pub async fn handle_normal_msg(
    handle: Arc<NetworkNodeHandle<CounterState>>,
    msg: CounterRequest,
    chan: Option<ResponseChannel<DirectMessageResponse>>,
) -> Result<(), NetworkNodeHandleError> {
    match msg {
        // direct message only
        CounterRequest::MyCounterIs(c) => {
            *handle.state.lock().await = c;
        }
        // gossip message only
        CounterRequest::IncrementCounter { from, to, .. } => {
            if *handle.state.lock().await == from {
                *handle.state.lock().await = to;
            }
        }
        // only as a response
        CounterRequest::AskForCounter => {
            if let Some(chan) = chan {
                let response = CounterRequest::MyCounterIs(*handle.state.lock().await);
                let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
                let serialized_response = bincode_options
                    .serialize(&response)
                    .context(SerializationSnafu)?;
                handle
                    .send_network
                    .send_async(ClientRequest::DirectResponse(chan, serialized_response))
                    .await
                    .context(SendSnafu)?
            }
        }
        CounterRequest::Kill => {
            handle.kill().await.context(NetworkSnafu)?;
        }
        CounterRequest::Recvd => {}
    }
    Ok(())
}

/// event handler for events from the swarm
/// - updates state based on events received
/// - replies to direct messages
#[instrument]
pub async fn regular_handle_network_event(
    event: NetworkEvent,
    handle: Arc<NetworkNodeHandle<CounterState>>,
) -> Result<(), NetworkNodeHandleError> {
    #[allow(clippy::enum_glob_use)]
    use NetworkEvent::*;
    let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
    match event {
        GossipMsg(m) | DirectResponse(m, _) => {
            if let Ok(msg) = bincode_options.deserialize::<Message>(&m) {
                match msg {
                    Message::NormalMessage(msg) => {
                        handle_normal_msg(handle.clone(), msg, None).await?;
                    }
                    Message::ConductorMessage(..) => {
                        // do nothing. We only expect to be reached out to by the conductor via
                        // direct message
                    }
                }
            }
        }
        DirectRequest(msg, chan) => {
            if let Ok(msg) = bincode_options.deserialize::<Message>(&msg) {
                match msg {
                    Message::NormalMessage(msg) => {
                        handle_normal_msg(handle.clone(), msg, Some(chan)).await?;
                    }
                    Message::ConductorMessage(msg, method) => {
                        let serialized_msg = bincode_options
                            .serialize(&Message::NormalMessage(msg))
                            .context(SerializationSnafu)?;
                        match method {
                            ConductorMessageMethod::Broadcast => {
                                handle
                                    .send_network
                                    .send_async(ClientRequest::GossipMsg(
                                        Topic::new("global"),
                                        serialized_msg,
                                    ))
                                    .await
                                    .context(SendSnafu)?;
                            }
                            ConductorMessageMethod::DirectMessage(pid) => {
                                handle
                                    .send_network
                                    .send_async(ClientRequest::DirectRequest(pid, serialized_msg))
                                    .await
                                    .context(SendSnafu)?;
                                let response = bincode_options
                                    .serialize(&Message::NormalMessage(CounterRequest::Recvd))
                                    .context(SerializationSnafu)?;
                                handle
                                    .send_network
                                    .send_async(ClientRequest::DirectResponse(chan, response))
                                    .await
                                    .context(SendSnafu)?;
                            }
                        }
                    }
                }
            }
        }
        UpdateConnectedPeers(p) => {
            handle.connection_state.lock().await.connected_peers = p;
        }
        UpdateKnownPeers(p) => {
            handle.connection_state.lock().await.known_peers = p;
        }
    }
    Ok(())
}

#[derive(StructOpt)]
pub struct CliOpt {
    /// Path to the node configuration file
    #[structopt(long = "index", short = "i")]
    pub idx: Option<usize>,
}

const NUM_NODES: usize = 2;
const PORT_NO: u16 = 1234;
const KNOWN_MULTIADDRS: [([u8; 4], u16); NUM_NODES] = [([0, 0, 0, 0], 1234), ([0, 0, 0, 0], 2345)];
const TIMEOUT: Duration = Duration::from_secs(1000);
static INIT: Once = Once::new();

pub async fn start_main_regular(
    idx: usize,
    node_type: NetworkNodeType,
) -> Result<(), NetworkNodeHandleError> {
    INIT.call_once(|| {
        color_eyre::install().unwrap();
        tracing_setup::setup_tracing();
    });
    let mut multiaddrs = Vec::<(Option<PeerId>, Multiaddr)>::new();
    for (ip, port) in KNOWN_MULTIADDRS {
        multiaddrs.push((None, build_multiaddr!(Ip4(ip), Tcp(port))));
    }
    let config = NetworkNodeConfigBuilder::default()
        .port(PORT_NO)
        .min_num_peers(KNOWN_MULTIADDRS.len() / 4)
        .max_num_peers(KNOWN_MULTIADDRS.len() / 2)
        .node_type(node_type)
        .build()
        .context(NodeConfigSnafu)?;
    let handle = spin_up_swarm::<CounterState>(TIMEOUT, multiaddrs, config, idx).await?;

    spawn_handler(handle, regular_handle_network_event).await;

    Ok(())
}

pub async fn start_main_controller(idx: usize) -> Result<(), NetworkNodeHandleError> {
    INIT.call_once(|| {
        color_eyre::install().unwrap();
        tracing_setup::setup_tracing();
    });
    let mut multiaddrs = Vec::<(Option<PeerId>, Multiaddr)>::new();
    for (ip, port) in KNOWN_MULTIADDRS {
        multiaddrs.push((None, build_multiaddr!(Ip4(ip), Tcp(port))));
    }
    let config = NetworkNodeConfigBuilder::default()
        .port(PORT_NO)
        .min_num_peers(KNOWN_MULTIADDRS.len() / 4)
        .max_num_peers(KNOWN_MULTIADDRS.len() / 2)
        .node_type(NetworkNodeType::Controller)
        .build()
        .context(NodeConfigSnafu)?;
    let handle = spin_up_swarm::<ControllerState>(TIMEOUT, multiaddrs, config, idx).await?;

    spawn_handler(handle, controller_handle_network_event).await;

    // FIXME
    // we need two primitives here:
    // - tell a node to tell all other nodes to increment state
    // - tell a node to broadcast increment state
    // then we need to update the state everywhere

    Ok(())
}

#[instrument]
pub async fn controller_handle_network_event(
    event: NetworkEvent,
    handle: Arc<NetworkNodeHandle<ControllerState>>,
) -> Result<(), NetworkNodeHandleError> {
    #[allow(clippy::enum_glob_use)]
    use NetworkEvent::*;
    let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
    match event {
        GossipMsg(..) | DirectRequest(..) => {
            // this node isn't going to participate.
            // it's only purpose is to recv state
        }
        DirectResponse(m, peer_id) => {
            if let Ok(msg) = bincode_options.deserialize::<Message>(&m) {
                match msg {
                    Message::NormalMessage(msg) => match msg {
                        CounterRequest::MyCounterIs(state) => {
                            let old_state = (*handle.state.lock().await)
                                .insert(peer_id, state)
                                .unwrap_or(0);
                            handle.state_changed.notify_all();
                            debug_assert!(old_state < state);
                        }
                        _ => {}
                    },
                    Message::ConductorMessage(..) => {
                        /* This should also never happen ... */
                        unreachable!()
                    }
                }
            }
        }
        // we care about these for the sake of maintaining conenctions, but not much else
        UpdateConnectedPeers(p) => {
            handle.connection_state.lock().await.connected_peers = p;
        }
        UpdateKnownPeers(p) => {
            handle.connection_state.lock().await.known_peers = p;
        }
    }
    Ok(())
}
