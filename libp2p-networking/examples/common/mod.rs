#[cfg(feature = "webui")]
pub mod web;

#[cfg(feature = "lossy_network")]
pub mod lossy_network;

use async_std::task::{sleep, spawn};

use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::{Arc, Once},
    time::Duration,
};

use libp2p::{gossipsub::Topic, multiaddr, request_response::ResponseChannel, Multiaddr, PeerId};
use networking_demo::{
    direct_message::DirectMessageResponse,
    network::{
        deserialize_msg,
        network_node_handle_error::{NetworkSnafu, NodeConfigSnafu, SerializationSnafu},
        serialize_msg, spawn_handler, spin_up_swarm, ClientRequest, NetworkEvent,
        NetworkNodeConfigBuilder, NetworkNodeHandle, NetworkNodeHandleError, NetworkNodeType,
    },
    tracing_setup,
};
use rand::{seq::IteratorRandom, thread_rng};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use std::fmt::Debug;
use structopt::StructOpt;
use tracing::instrument;

#[cfg(feature = "webui")]
use std::net::SocketAddr;

const TIMEOUT: Duration = Duration::from_secs(1000);
static INIT: Once = Once::new();

pub type CounterState = u32;
pub type ConductorState = HashMap<PeerId, CounterState>;

#[cfg(feature = "webui")]
impl web::WebInfo for ConductorState {
    type Serialized = serde_json::Value;

    fn get_serializable(&self) -> Self::Serialized {
        let mut map = serde_json::map::Map::new();
        for (peer, state) in self.iter() {
            map.insert(peer.to_base58(), (*state).into());
        }
        serde_json::Value::Object(map)
    }
}

#[cfg(feature = "webui")]
impl web::WebInfo for CounterState {
    type Serialized = u32;
    fn get_serializable(&self) -> Self::Serialized {
        *self
    }
}

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
    Normal(CounterRequest),
    /// message a conductor sent to a node
    /// that the node must send to other node(s)
    Conductor(CounterRequest, ConductorMessageMethod),
    // announce the conductor
    ConductorIdIs(PeerId),
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
            handle.modify_state(|s| *s = c).await;
        }
        // gossip message only
        CounterRequest::IncrementCounter { from, to, .. } => {
            handle
                .modify_state(|s| {
                    if *s == from {
                        *s = to
                    }
                })
                .await;
            // if *handle.state.lock().await == from {
            //     *handle.state.lock().await = to;
            //     handle.notify_webui().await;
            // }
        }
        // only as a response
        CounterRequest::AskForCounter => {
            if let Some(chan) = chan {
                let response = Message::Normal(CounterRequest::MyCounterIs(handle.state().await));
                let serialized_response = serialize_msg(&response).context(SerializationSnafu)?;
                println!("sending back reponse: {:?})", response);
                handle
                    .send_request(ClientRequest::DirectResponse(chan, serialized_response))
                    .await?;
            }
        }
        CounterRequest::Kill => {
            println!("killing!");
            handle.shutdown().await.context(NetworkSnafu)?;
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
                    Message::ConductorIdIs(peerid) => {
                        handle
                            .send_request(ClientRequest::IgnorePeers(vec![peerid]))
                            .await?;
                        handle.add_ignored_peer(peerid).await;
                    }
                    Message::Normal(msg) => {
                        handle_normal_msg(handle.clone(), msg, None).await?;
                    }
                    Message::Conductor(..) => {
                        // do nothing. We only expect to be reached out to by the conductor via
                        // direct message
                    }
                }
            }
        }
        DirectRequest(msg, _peer_id, chan) => {
            if let Ok(msg) = deserialize_msg::<Message>(&msg) {
                match msg {
                    Message::ConductorIdIs(_) => {}
                    Message::Normal(msg) => {
                        println!("recv-ed normal direct message {:?}", msg);
                        handle_normal_msg(handle.clone(), msg, Some(chan)).await?;
                    }
                    Message::Conductor(msg, method) => {
                        println!("recv-ed conductor message!");
                        let serialized_msg =
                            serialize_msg(&Message::Normal(msg)).context(SerializationSnafu)?;
                        match method {
                            ConductorMessageMethod::Broadcast => {
                                handle
                                    .send_request(ClientRequest::GossipMsg(
                                        Topic::new("global"),
                                        serialized_msg,
                                    ))
                                    .await?;
                            }
                            ConductorMessageMethod::DirectMessage(pid) => {
                                handle
                                    .send_request(ClientRequest::DirectRequest(pid, serialized_msg))
                                    .await?;
                                let response =
                                    serialize_msg(&Message::Normal(CounterRequest::Recvd))
                                        .context(SerializationSnafu)?;
                                handle
                                    .send_request(ClientRequest::DirectResponse(chan, response))
                                    .await?;
                            }
                        }
                    }
                }
            }
        }
        UpdateConnectedPeers(p) => {
            handle.set_connected_peers(p).await;
            handle.notify_webui().await;
        }
        UpdateKnownPeers(p) => {
            handle.set_known_peers(p).await;
            handle.notify_webui().await;
        }
    }
    Ok(())
}

pub fn parse_node(s: &str) -> Result<Multiaddr, multiaddr::Error> {
    let mut i = s.split(':');
    let ip = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    let port = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    Multiaddr::from_str(&format!("/ip4/{}/tcp/{}", ip, port))
}

#[derive(StructOpt)]
pub struct CliOpt {
    /// Path to the node configuration file
    /// only should be provided for conductor node
    // #[structopt(long = "inventory", short = "i")]
    // pub inventory: Option<String>,
    #[structopt(long = "bootstrap")]
    #[structopt(parse(try_from_str = parse_node))]
    pub bootstrap_addrs: Vec<Multiaddr>,
    #[structopt(long = "num_nodes")]
    pub num_nodes: usize,
    #[structopt(long = "node_type")]
    pub node_type: NetworkNodeType,
    #[structopt(long = "bound_addr")]
    #[structopt(parse(try_from_str = parse_node))]
    pub bound_addr: Multiaddr,
    #[cfg(feature = "webui")]
    /// If this value is set, a webserver will be spawned on this address with debug info
    #[structopt(long = "webui")]
    pub webui_addr: Option<SocketAddr>,
    #[cfg(feature = "lossy_network")]
    #[structopt(long = "env")]
    pub env_type: ExecutionEnvironment,
}

/// The execution environemnt type
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(feature = "lossy_network")]
pub enum ExecutionEnvironment {
    /// execution environment is within docker
    Docker,
    /// execution environment is on metal
    Metal,
}

#[cfg(feature = "lossy_network")]
impl FromStr for ExecutionEnvironment {
    type Err = String;

    fn from_str(input: &str) -> Result<ExecutionEnvironment, Self::Err> {
        match input {
            "Docker" => Ok(ExecutionEnvironment::Docker),
            "Metal" => Ok(ExecutionEnvironment::Metal),
            _ => Err(
                "Couldn't parse execution environment. Must be one of Metal, Docker".to_string(),
            ),
        }
    }
}

/// ['bootstrap_addrs`] list of bootstrap multiaddrs. Needed to bootstrap into network
/// [`num_nodes`] total number of nodes. Needed to create pruning rules
/// [`node_type`] the type of this node
/// ['bound_addr`] the address to bind to
pub async fn start_main(opts: CliOpt) -> Result<(), CounterError> {
    // FIXME can we pass in a function that returns an error type
    INIT.call_once(|| {
        color_eyre::install().unwrap();
        tracing_setup::setup_tracing();
    });
    let bootstrap_nodes = opts
        .bootstrap_addrs
        .iter()
        .cloned()
        .map(|a| (None, a))
        .collect::<Vec<_>>();

    match opts.node_type {
        NetworkNodeType::Conductor => {
            let config = NetworkNodeConfigBuilder::default()
                .bound_addr(opts.bound_addr)
                .min_num_peers(opts.num_nodes - 1)
                .max_num_peers(opts.num_nodes - 1)
                .node_type(NetworkNodeType::Conductor)
                .ignored_peers(HashSet::new())
                .build()
                .context(NodeConfigSnafu)
                .context(HandleSnafu)?;
            let handle = Arc::new(
                NetworkNodeHandle::<ConductorState>::new(config.clone(), 0)
                    .await
                    .context(HandleSnafu)?,
            );
            #[cfg(feature = "webui")]
            if let Some(addr) = opts.webui_addr {
                web::spawn_server(Arc::clone(&handle), addr);
            }

            spin_up_swarm(TIMEOUT, bootstrap_nodes, config, 0, &handle)
                .await
                .context(HandleSnafu)?;

            spawn_handler(handle.clone(), conductor_handle_network_event).await;

            let mut state = handle.state_lock().await;
            for a_peer in handle.known_peers().await {
                if a_peer != handle.peer_id() && state.get(&a_peer).is_none() {
                    state.insert(a_peer, 0);
                }
            }
            drop(state);
            handle.notify_webui().await;

            let handle_dup = handle.clone();
            let conductor_peerid = handle.peer_id();
            // the "conductor id"
            // periodically say "ignore me!"
            spawn(async move {
                let msg = Message::ConductorIdIs(conductor_peerid);
                let serialized_msg = serialize_msg(&msg)
                    .context(SerializationSnafu)
                    .context(HandleSnafu)?;
                while !handle_dup.is_killed().await {
                    sleep(Duration::from_secs(1)).await;
                    handle_dup
                        .send_request(ClientRequest::GossipMsg(
                            Topic::new("global"),
                            serialized_msg.clone(),
                        ))
                        .await
                        .context(HandleSnafu)?;
                }
                Ok::<(), CounterError>(())
            });

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

            let kill_msg = Message::Normal(CounterRequest::Kill);
            let serialized_kill_msg = serialize_msg(&kill_msg)
                .context(SerializationSnafu)
                .context(HandleSnafu)?;
            for peer_id in handle.connected_peers().await {
                handle
                    .send_request(ClientRequest::DirectRequest(
                        peer_id,
                        serialized_kill_msg.clone(),
                    ))
                    .await
                    .context(HandleSnafu)?
            }

            while !handle.connected_peers().await.is_empty() {
                async_std::task::sleep(Duration::from_millis(100)).await;
            }
        }
        // regular and bootstrap nodes
        NetworkNodeType::Regular | NetworkNodeType::Bootstrap => {
            let config = NetworkNodeConfigBuilder::default()
                .bound_addr(opts.bound_addr)
                .ignored_peers(HashSet::new())
                .min_num_peers(opts.num_nodes / 4)
                .max_num_peers(opts.num_nodes / 2)
                .node_type(opts.node_type)
                .build()
                .context(NodeConfigSnafu)
                .context(HandleSnafu)?;
            let handle = Arc::new(
                NetworkNodeHandle::<CounterState>::new(config.clone(), 0)
                    .await
                    .context(HandleSnafu)?,
            );
            #[cfg(feature = "webui")]
            if let Some(addr) = opts.webui_addr {
                web::spawn_server(Arc::clone(&handle), addr);
            }

            spin_up_swarm(TIMEOUT, bootstrap_nodes, config, 0, &handle)
                .await
                .context(HandleSnafu)?;
            let handle_dup = handle.clone();
            // periodically broadcast state back to conductor node
            spawn(async move {
                while !handle_dup.is_killed().await {
                    if let Some(conductor_id) = handle_dup.ignored_peers().await.iter().next() {
                        let counter = handle_dup.state().await;
                        let msg = Message::Normal(CounterRequest::MyCounterIs(counter));
                        let serialized_msg = serialize_msg(&msg)
                            .context(SerializationSnafu)
                            .context(HandleSnafu)?;
                        handle_dup
                            .send_request(ClientRequest::DirectRequest(
                                *conductor_id,
                                serialized_msg,
                            ))
                            .await
                            .context(HandleSnafu)?;
                    }
                    sleep(Duration::from_secs(1)).await;
                }
                Ok::<(), CounterError>(())
            });
            spawn_handler(handle.clone(), regular_handle_network_event).await;
            while !handle.is_killed().await {
                async_std::task::sleep(Duration::from_millis(100)).await;
            }
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
    let mut known_peers = handle.known_peers().await;
    known_peers.remove(&handle.peer_id());

    // FIXME wrapper error
    let chosen_peer = known_peers.iter().choose(&mut thread_rng()).unwrap();
    println!("chosen peer is {:?}", chosen_peer);

    // step 1: increment counter on the chosen node

    // set up listener before any state has the chance to change
    let res_fut =
        handle
            .state_changed()
            .wait_timeout_until(handle.state_lock().await, timeout, |state| {
                *state.get(chosen_peer).unwrap() == new_state
            });

    // dispatch message
    let msg = Message::Normal(CounterRequest::IncrementCounter {
        from: state,
        to: new_state,
    });
    let serialized_msg = serialize_msg(&msg).context(SerializationSnafu)?;
    handle
        .send_request(ClientRequest::DirectRequest(*chosen_peer, serialized_msg))
        .await?;

    if res_fut.await.1.timed_out() {
        panic!("timeout!");
    }

    println!("step 1 is complete!");

    // step 2: iterate through remaining nodes, message them "request state from chosen node"

    // set up listener first
    let res_fut =
        handle
            .state_changed()
            .wait_timeout_until(handle.state_lock().await, timeout, |state| {
                state.iter().all(|(_, &s)| s == new_state)
            });

    // send out the requests to ask the chosen peer for its state (and replace ours)

    let mut remaining_nodes = known_peers.clone();
    remaining_nodes.remove(chosen_peer);

    for peer in &remaining_nodes {
        let msg = Message::Conductor(
            CounterRequest::AskForCounter,
            ConductorMessageMethod::DirectMessage(*chosen_peer),
        );
        let serialized_msg = serialize_msg(&msg).context(SerializationSnafu)?;
        handle
            .send_request(ClientRequest::DirectRequest(*peer, serialized_msg))
            .await?;
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
    let mut known_peers = handle.known_peers().await;
    known_peers.remove(&handle.peer_id());

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
    let msg = Message::Conductor(request.clone(), ConductorMessageMethod::Broadcast);
    let serialized_msg = serialize_msg(&msg).context(SerializationSnafu)?;
    handle
        .send_request(ClientRequest::DirectRequest(*chosen_peer, serialized_msg))
        .await?;

    // set up listener before any state has the chance to change
    let res_fut =
        handle
            .state_changed()
            .wait_timeout_until(handle.state_lock().await, timeout, |state| {
                state.iter().all(|(_, &s)| s == new_state)
            });

    let msg_direct = Message::Normal(request);
    let serialized_msg_direct = serialize_msg(&msg_direct).context(SerializationSnafu)?;
    handle
        .send_request(ClientRequest::DirectRequest(
            *chosen_peer,
            serialized_msg_direct,
        ))
        .await?;

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
                    Message::Normal(msg) => {
                        if let CounterRequest::MyCounterIs(state) = msg {
                            handle
                                .modify_state(|s| {
                                    s.insert(peer_id, state);
                                    println!("new state: {:?}", s);
                                })
                                .await;
                        }
                    }
                    Message::Conductor(..) => {
                        /* This should also never happen ... */
                        unreachable!()
                    }
                    Message::ConductorIdIs(_) => {}
                }
            }
        }
        DirectResponse(_m, _peer_id) => { /* nothing to do here */ }
        // we care about these for the sake of maintaining conenctions, but not much else
        UpdateConnectedPeers(p) => {
            handle.set_connected_peers(p).await;
            handle.notify_webui().await;
        }
        UpdateKnownPeers(p) => {
            handle.set_known_peers(p).await;
            handle.notify_webui().await;
        }
    }
    Ok(())
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum CounterError {
    Handle { source: NetworkNodeHandleError },
    FileRead { source: std::io::Error },
    MissingBootstrap,
}
