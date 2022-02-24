use networking_demo::parse_config::NodeDescription;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Once},
    time::Duration,
};

use async_std::{
    fs::File,
    task::{sleep, spawn},
};
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
use snafu::{ResultExt, Snafu};
use std::fmt::Debug;
use structopt::StructOpt;
use tracing::instrument;

const TIMEOUT: Duration = Duration::from_secs(1000);
static INIT: Once = Once::new();

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
                let response =
                    Message::NormalMessage(CounterRequest::MyCounterIs(*handle.state.lock().await));
                let serialized_response = serialize_msg(&response).context(SerializationSnafu)?;
                println!("sending back reponse: {:?})", response);
                handle
                    .send_network
                    .send_async(ClientRequest::DirectResponse(chan, serialized_response))
                    .await
                    .context(SendSnafu)?
            }
        }
        CounterRequest::Kill => {
            println!("killing!");
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
        DirectRequest(msg, _peer_id, chan) => {
            if let Ok(msg) = deserialize_msg::<Message>(&msg) {
                match msg {
                    Message::NormalMessage(msg) => {
                        println!("recv-ed normal direct message {:?}", msg);
                        handle_normal_msg(handle.clone(), msg, Some(chan)).await?;
                    }
                    Message::ConductorMessage(msg, method) => {
                        println!("recv-ed conductor message!");
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
    #[structopt(long = "ip_addr", short = "ip")]
    pub ip: Option<String>,
    #[structopt(long = "toplogy_path", short = "path")]
    pub path: Option<String>,
}

pub async fn parse_config(path: Option<String>) -> Result<Vec<NodeDescription>, CounterError> {
    let mut f = File::open(&path.unwrap_or_else(|| "./identity_mapping.json".to_string()))
        .await
        .context(FileReadSnafu)?;
    let mut s = String::new();
    f.read_to_string(&mut s).await.context(FileReadSnafu)?;
    serde_json::from_str(&s).context(JsonParseSnafu)
}

pub async fn start_main(ip_addr: String, path: Option<String>) -> Result<(), CounterError> {
    // FIXME can we pass in a function that returns an error type
    INIT.call_once(|| {
        color_eyre::install().unwrap();
        tracing_setup::setup_tracing();
    });
    let swarm_config = parse_config(path).await?;

    let ignored_peers = swarm_config
        .iter()
        .filter_map(|n| {
            if n.node_type == NetworkNodeType::Conductor {
                Some(n.identity.public().to_peer_id())
            } else {
                None
            }
        })
        .collect::<HashSet<_>>();

    let (idx, node_description) = swarm_config.iter().enumerate().find(|(_, node)| {
        node.multiaddr.clone().to_string().contains(&ip_addr)
    }).unwrap();
    println!("found entry!: {idx}");

    match node_description.node_type {
        NetworkNodeType::Conductor => {
            let config = NetworkNodeConfigBuilder::default()
                .bound_addr(Some(node_description.bound_addr.clone()))
                .min_num_peers(swarm_config.len() - 1)
                .max_num_peers(swarm_config.len() - 1)
                .node_type(NetworkNodeType::Conductor)
                .identity(Some(node_description.identity.clone()))
                .ignored_peers(ignored_peers)
                .build()
                .context(NodeConfigSnafu)
                .context(HandleSnafu)?;
            let handle = spin_up_swarm::<ConductorState>(
                TIMEOUT,
                swarm_config
                    .iter()
                    .map(|c| (Some(c.identity.public().to_peer_id()), c.multiaddr.clone()))
                    .collect::<Vec<_>>(),
                config,
                idx,
            )
            .await
            .context(HandleSnafu)?;
            spawn_handler(handle.clone(), conductor_handle_network_event).await;

            // initialize the state of each node
            let mut state = handle.state.lock().await;
            for (i, connection) in swarm_config.iter().enumerate() {
                if i != idx {
                    state.insert(connection.identity.public().to_peer_id(), 0);
                }
            }
            drop(state);

            for i in 0..5 {
                conductor_broadcast(TIMEOUT, i, handle.clone())
                    .await
                    .context(HandleSnafu)?
            }

            for j in 5..10 {
                conductor_direct_message(TIMEOUT, j, handle.clone())
                    .await
                    .context(HandleSnafu)?
            }

            let kill_msg = Message::NormalMessage(CounterRequest::Kill);
            let serialized_kill_msg = serialize_msg(&kill_msg)
                .context(SerializationSnafu)
                .context(HandleSnafu)?;
            for peer_id in &handle.connection_state.lock().await.connected_peers {
                handle
                    .send_network
                    .send_async(ClientRequest::DirectRequest(
                        *peer_id,
                        serialized_kill_msg.clone(),
                    ))
                    .await
                    .context(SendSnafu)
                    .context(HandleSnafu)?
            }
            while !handle
                .connection_state
                .lock()
                .await
                .connected_peers
                .is_empty()
            {}

            // FIXME
            // we need one other primitive here:
            // - tell a node to tell all other nodes to increment state with direct message
        }
        NetworkNodeType::Bootstrap | NetworkNodeType::Regular => {
            let known_peers = swarm_config
                .iter()
                .filter_map(|x| {
                    // TODO this is gross. Make this a data structure
                    if x.node_type == NetworkNodeType::Bootstrap {
                        Some((Some(x.identity.public().to_peer_id()), x.bound_addr.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let config = NetworkNodeConfigBuilder::default()
                .bound_addr(Some(node_description.bound_addr.clone()))
                .identity(Some(node_description.identity.clone()))
                .ignored_peers(ignored_peers)
                .min_num_peers(swarm_config.len() / 4)
                .max_num_peers(swarm_config.len() / 2)
                .node_type(node_description.node_type)
                .build()
                .context(NodeConfigSnafu)
                .context(HandleSnafu)?;
            let handle = spin_up_swarm::<CounterState>(TIMEOUT, known_peers, config, idx)
                .await
                .context(HandleSnafu)?;
            let handle_dup = handle.clone();
            // periodically broadcast state back to conductor node
            spawn(async move {
                // FIXME map option to error
                let conductor_id = swarm_config
                    .iter()
                    .find(|c| c.node_type == NetworkNodeType::Conductor)
                    .unwrap()
                    .identity
                    .public()
                    .to_peer_id();
                while !*handle_dup.killed.lock().await {
                    sleep(Duration::from_secs(1)).await;
                    let counter = *handle_dup.state.lock().await;
                    let msg = Message::NormalMessage(CounterRequest::MyCounterIs(counter));
                    let serialized_msg = serialize_msg(&msg)
                        .context(SerializationSnafu)
                        .context(HandleSnafu)?;
                    handle_dup
                        .send_network
                        .send_async(ClientRequest::DirectRequest(conductor_id, serialized_msg))
                        .await
                        .context(SendSnafu)
                        .context(HandleSnafu)?;
                }
                Ok::<(), CounterError>(())
            });
            spawn_handler(handle.clone(), regular_handle_network_event).await;
            while !*handle.killed.lock().await {}
        }
    }

    Ok(())
}

pub async fn conductor_direct_message(
    timeout: Duration,
    state: CounterState,
    handle: Arc<NetworkNodeHandle<ConductorState>>,
) -> Result<(), NetworkNodeHandleError> {
    // new state
    let new_state = state + 1;

    // pick a peer to do the be the recipient of the direct messages
    let mut known_peers = handle.connection_state.lock().await.known_peers.clone();
    known_peers.remove(&handle.peer_id);

    // FIXME wrapper error
    let chosen_peer = known_peers.iter().choose(&mut thread_rng()).unwrap();
    println!("chosen peer is {:?}", chosen_peer);

    // step 1: increment counter on the chosen node

    // set up listener before any state has the chance to change
    let res_fut =
        handle
            .state_changed
            .wait_timeout_until(handle.state.lock().await, timeout, |state| {
                *state.get(chosen_peer).unwrap() == new_state
            });

    // dispatch message
    let msg = Message::NormalMessage(CounterRequest::IncrementCounter {
        from: state,
        to: new_state,
    });
    let serialized_msg = serialize_msg(&msg).context(SerializationSnafu)?;
    handle
        .send_network
        .send_async(ClientRequest::DirectRequest(*chosen_peer, serialized_msg))
        .await
        .context(SendSnafu)?;

    if res_fut.await.1.timed_out() {
        panic!("timeout!");
    }

    println!("step 1 is complete!");

    // step 2: iterate through remaining nodes, message them "request state from chosen node"

    // set up listener first
    let res_fut =
        handle
            .state_changed
            .wait_timeout_until(handle.state.lock().await, timeout, |state| {
                state.iter().all(|(_, &s)| s == new_state)
            });

    // send out the requests to ask the chosen peer for its state (and replace ours)

    let mut remaining_nodes = known_peers.clone();
    remaining_nodes.remove(&chosen_peer);

    for peer in &remaining_nodes {
        let msg = Message::ConductorMessage(
            CounterRequest::AskForCounter,
            ConductorMessageMethod::DirectMessage(*chosen_peer),
        );
        let serialized_msg = serialize_msg(&msg).context(SerializationSnafu)?;
        handle
            .send_network
            .send_async(ClientRequest::DirectRequest(*peer, serialized_msg))
            .await
            .context(SendSnafu)?;
    }

    if res_fut.await.1.timed_out() {
        panic!("timeout!");
    } else {
        Ok(())
    }
}

pub async fn conductor_broadcast(
    timeout: Duration,
    state: CounterState,
    handle: Arc<NetworkNodeHandle<ConductorState>>,
) -> Result<(), NetworkNodeHandleError> {
    let new_state = state + 1;
    let mut known_peers = handle.connection_state.lock().await.known_peers.clone();
    known_peers.remove(&handle.peer_id);

    // FIXME wrapper error
    let chosen_peer = known_peers.iter().choose(&mut thread_rng()).unwrap();
    println!("chosen peer is {:?}", chosen_peer);

    // increment the state
    let request = CounterRequest::IncrementCounter {
        from: state,
        to: new_state,
    };
    println!("broadcasting message!");
    // broadcast message
    let msg = Message::ConductorMessage(request.clone(), ConductorMessageMethod::Broadcast);
    let serialized_msg = serialize_msg(&msg).context(SerializationSnafu)?;
    handle
        .send_network
        .send_async(ClientRequest::DirectRequest(*chosen_peer, serialized_msg))
        .await
        .context(SendSnafu)?;

    // set up listener before any state has the chance to change
    let res_fut =
        handle
            .state_changed
            .wait_timeout_until(handle.state.lock().await, timeout, |state| {
                state.iter().all(|(_, &s)| s == new_state)
            });

    let msg_direct = Message::NormalMessage(request);
    let serialized_msg_direct = serialize_msg(&msg_direct).context(SerializationSnafu)?;
    handle
        .send_network
        .send_async(ClientRequest::DirectRequest(
            *chosen_peer,
            serialized_msg_direct,
        ))
        .await
        .context(SendSnafu)?;

    if res_fut.await.1.timed_out() {
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
        GossipMsg(..) => {
            // this node isn't going to participate in gossip/dms to update state
            // it's only purpose is to recv state
        }
        DirectRequest(m, peer_id, _chan) => {
            if let Ok(msg) = deserialize_msg::<Message>(&m) {
                match msg {
                    Message::NormalMessage(msg) => {
                        if let CounterRequest::MyCounterIs(state) = msg {
                            let _old_state = (*handle.state.lock().await)
                                .insert(peer_id, state)
                                .unwrap_or(0);
                            println!("new state: {:?}", *handle.state.lock().await);
                            handle.state_changed.notify_all();
                        }
                    }
                    Message::ConductorMessage(..) => {
                        /* This should also never happen ... */
                        unreachable!()
                    }
                }
            }
        }
        DirectResponse(_m, _peer_id) => { /* nothing to do here */ }
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

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum CounterError {
    Handle { source: NetworkNodeHandleError },
    FileRead { source: std::io::Error },
    JsonParse { source: serde_json::Error },
    MissingBootstrap,
}
