use networking_demo::parse_config::NodeDescription;
use std::{
    collections::HashMap,
    sync::{Arc, Once},
    time::Duration,
};

use async_std::fs::File;
use futures::AsyncReadExt;
use libp2p::{gossipsub::Topic, request_response::ResponseChannel, PeerId};
use networking_demo::{
    direct_message::DirectMessageResponse,
    network_node::{
        deserialize_msg, serialize_msg, ClientRequest, NetworkEvent, NetworkNodeConfigBuilder,
        NetworkNodeType,
    },
    network_node_handle::{
        spawn_handler, spin_up_swarm, NetworkNodeHandle, NetworkNodeHandleError, NetworkSnafu,
        NodeConfigSnafu, SendSnafu, SerializationSnafu,
    },
    tracing_setup,
};
use rand::{seq::IteratorRandom, thread_rng};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::fmt::Debug;
use structopt::StructOpt;
use tracing::instrument;

const TIMEOUT: Duration = Duration::from_secs(1000);
static INIT: Once = Once::new();
const PORT_NO: u16 = 1234;

pub type CounterState = u32;
pub type ConductorState = HashMap<PeerId, CounterState>;

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
                let serialized_response = serialize_msg(&response).context(SerializationSnafu)?;
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
    match event {
        GossipMsg(m) | DirectResponse(m, _) => {
            if let Ok(msg) = deserialize_msg::<Message>(&m) {
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
            if let Ok(msg) = deserialize_msg::<Message>(&msg) {
                match msg {
                    Message::NormalMessage(msg) => {
                        handle_normal_msg(handle.clone(), msg, Some(chan)).await?;
                    }
                    Message::ConductorMessage(msg, method) => {
                        let serialized_msg = serialize_msg(&Message::NormalMessage(msg))
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
                                let response =
                                    serialize_msg(&Message::NormalMessage(CounterRequest::Recvd))
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

pub async fn parse_config() -> Vec<NodeDescription> {
    let mut f = File::open(&"./identity_mapping.json").await.unwrap();
    let mut s = String::new();
    f.read_to_string(&mut s).await.unwrap();
    let res: Vec<NodeDescription> = serde_json::from_str(&s).unwrap();
    res
}

pub async fn start_main(idx: usize) -> Result<(), NetworkNodeHandleError> {
    INIT.call_once(|| {
        color_eyre::install().unwrap();
        tracing_setup::setup_tracing();
    });
    let swarm_config = parse_config().await;

    let node_description = &swarm_config[idx];

    match node_description.node_type {
        NetworkNodeType::Conductor => {
            let config = NetworkNodeConfigBuilder::default()
                .port(PORT_NO)
                .min_num_peers(swarm_config.len() - 1)
                .max_num_peers(swarm_config.len() - 1)
                .node_type(NetworkNodeType::Conductor)
                .identity(Some(swarm_config[idx].identity.clone()))
                .build()
                .context(NodeConfigSnafu)?;
            let handle = spin_up_swarm::<ConductorState>(
                TIMEOUT,
                swarm_config
                    .iter()
                    .map(|c| (Some(c.identity.public().to_peer_id()), c.multiaddr.clone()))
                    .collect::<Vec<_>>(),
                config,
                idx,
            )
            .await?;
            spawn_handler(handle.clone(), conductor_handle_network_event).await;
            let mut state = handle.state.lock().await;
            for (i, connection) in swarm_config.iter().enumerate() {
                if i != idx {
                    state.insert(connection.identity.public().to_peer_id(), 0);
                }
            }
            drop(state);

            // FIXME we are in desperate need of a error type like TestError
            conductor_broadcast(TIMEOUT, 0, handle).await.unwrap();

            // FIXME we need to fix clippy warnings

            // FIXME we need to fix the serialization to do error handling properly

            // FIXME we need to convince the inner networking implementation to ignore the conductor nodes

            // FIXME
            // we need one other primitive here:
            // - tell a node to tell all other nodes to increment state with direct message

            // FIXME we have a bunch of nodes all on the same port. Don't use global variable for
            // this.
        }
        _ => {
            let known_peers = swarm_config
                .iter()
                .filter_map(|x| {
                    // TODO this is gross. Make this a data structure
                    if x.node_type == NetworkNodeType::Bootstrap {
                        Some((Some(x.identity.public().to_peer_id()), x.multiaddr.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let config = NetworkNodeConfigBuilder::default()
                .port(PORT_NO)
                .min_num_peers(swarm_config.len() / 4)
                .max_num_peers(swarm_config.len() / 2)
                .node_type(node_description.node_type)
                .build()
                .context(NodeConfigSnafu)?;
            let handle = spin_up_swarm::<CounterState>(TIMEOUT, known_peers, config, idx).await?;
            spawn_handler(handle, regular_handle_network_event).await;
        }
    }

    Ok(())
}

pub async fn conductor_broadcast(
    timeout: Duration,
    state: CounterState,
    handle: Arc<NetworkNodeHandle<ConductorState>>,
) -> Result<(), NetworkNodeHandleError> {
    // start listener future with timeout
    //
    let new_state = state + 1;
    //
    let mut known_peers = handle.connection_state.lock().await.known_peers.clone();
    known_peers.remove(&handle.peer_id);

    // FIXME wrapper error
    let chosen_peer = known_peers.iter().choose(&mut thread_rng()).unwrap();

    // increment the state
    let request = CounterRequest::IncrementCounter {
        from: state,
        to: new_state,
    };
    // broadcast message
    let msg = Message::ConductorMessage(request, ConductorMessageMethod::Broadcast);
    let serialized_msg = serialize_msg(&msg).context(SerializationSnafu)?;
    handle
        .send_network
        .send_async(ClientRequest::DirectRequest(*chosen_peer, serialized_msg))
        .await
        .context(SendSnafu)?;

    let (_, res) = handle
        .state_changed
        .wait_timeout_until(handle.state.lock().await, timeout, |state| {
            state.iter().all(|(_, &s)| s == new_state)
        })
        .await;

    if res.timed_out() {
        panic!("timeout!");
    } else {
        Ok(())
    }
}

#[instrument]
pub async fn conductor_handle_network_event(
    event: NetworkEvent,
    handle: Arc<NetworkNodeHandle<ConductorState>>,
) -> Result<(), NetworkNodeHandleError> {
    #[allow(clippy::enum_glob_use)]
    use NetworkEvent::*;
    match event {
        GossipMsg(..) | DirectRequest(..) => {
            // this node isn't going to participate.
            // it's only purpose is to recv state
        }
        DirectResponse(m, peer_id) => {
            if let Ok(msg) = deserialize_msg::<Message>(&m) {
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
