#[cfg(feature = "webui")]
pub mod web;

#[cfg(all(feature = "lossy_network", target_os = "linux"))]
pub mod lossy_network;

use async_std::{
    prelude::StreamExt,
    task::{sleep, spawn},
};
use hotshot_utils::test_util::{setup_backtrace, setup_logging};

use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime},
};

use libp2p::{multiaddr, request_response::ResponseChannel, Multiaddr, PeerId};
use libp2p_networking::{
    direct_message::DirectMessageResponse,
    network::{
        deserialize_msg, network_node_handle_error::NodeConfigSnafu, spawn_handler, spin_up_swarm,
        NetworkEvent, NetworkNodeConfigBuilder, NetworkNodeHandle, NetworkNodeHandleError,
        NetworkNodeType,
    },
};
use rand::{seq::IteratorRandom, thread_rng};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use std::fmt::Debug;
use structopt::StructOpt;
use tracing::{error, info, instrument};

#[cfg(feature = "webui")]
use std::net::SocketAddr;

const TIMEOUT: Duration = Duration::from_secs(1000);
const PADDING_SIZE: usize = 512;

pub type CounterState = u32;
pub type Epoch = (CounterState, CounterState);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpochType {
    BroadcastViaGossip,
    BroadcastViaDM,
    DMViaDM,
}

#[derive(Debug, Clone)]
pub struct ConductorState {
    ready_set: HashSet<PeerId>,
    current_epoch: EpochData,
    previous_epochs: HashMap<Epoch, EpochData>,
}

#[derive(Debug, Clone)]
pub struct EpochData {
    epoch_idx: Epoch,
    epoch_type: EpochType,
    node_states: HashMap<PeerId, CounterState>,
    message_durations: Vec<Duration>,
}

impl EpochData {
    pub fn increment_epoch(&mut self) {
        self.epoch_idx = (self.epoch_idx.1, self.epoch_idx.1 + 1)
    }
}

impl Default for ConductorState {
    fn default() -> Self {
        Self {
            ready_set: Default::default(),
            current_epoch: EpochData {
                epoch_idx: (0, 1),
                epoch_type: EpochType::BroadcastViaGossip,
                node_states: Default::default(),
                message_durations: Default::default(),
            },
            previous_epochs: Default::default(),
        }
    }
}

impl ConductorState {
    /// Increment conductor to the next epoch
    pub fn complete_round(&mut self, next_epoch_type: EpochType) {
        let current_epoch = self.current_epoch.clone();
        self.previous_epochs
            .insert(current_epoch.epoch_idx, current_epoch);
        self.current_epoch.epoch_type = next_epoch_type;
        self.current_epoch.message_durations = Default::default();
        self.current_epoch.increment_epoch();
    }
}

#[cfg(feature = "webui")]
impl web::WebInfo for ConductorState {
    type Serialized = serde_json::Value;

    fn get_serializable(&self) -> Self::Serialized {
        let mut map = serde_json::map::Map::new();
        for (peer, state) in self.current_epoch.node_states.iter() {
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

/// Normal message. Sent amongst [`NetworkNodeType::Regular`] and [`NetworkNodeType::Bootstrap`] nodes
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum CounterRequest {
    /// Request state
    StateRequest,
    /// Reply with state
    StateResponse(CounterState),
    /// kill node
    Kill,
}

/// Message sent between non-[`NetworkNodeType::Conductor`] nodes
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct NormalMessage {
    /// timestamp when message was sent
    sent_ts: SystemTime,
    /// whether or not message shuld be relayed to conductor
    relay_to_conductor: bool,
    /// the underlying request the recv-ing node should take
    req: CounterRequest,
    /// the epoch the message was sent on
    epoch: (CounterState, CounterState),
    /// arbitrary amount of padding to vary message length
    padding: Vec<u64>,
}

/// A message sent and recv-ed by a ['NetworkNodeType::Regular'] or ['NetworkNodeType::Bootstrap'] node
/// that is to be relayed back to a [`NetworkNodeType::Conductor`] node
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct RelayedMessage {
    /// time message took to propagate from sender to recv-er
    duration: Duration,
    /// the requeset being made
    req: CounterRequest,
    /// the epoch the request was made on
    epoch: (CounterState, CounterState),
}

/// A message sent and recv-ed by a ['NetworkNodeType::Regular'] or ['NetworkNodeType::Bootstrap'] node
/// that is to be relayed back to a [`NetworkNodeType::Conductor`] node
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ConductorMessage {
    /// the requeset the recv-ing node should make
    req: CounterRequest,
    /// the type of broadcast (direct or broadcast)
    broadcast_type: ConductorMessageMethod,
}

/// overall message
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum Message {
    /// message to end from a peer to a peer
    Normal(NormalMessage),
    /// messaged recved and relayed to conductor
    Relayed(RelayedMessage),
    /// conductor requests that message is sent to node
    /// that the node must send to other node(s)
    Conductor(ConductorMessage),
    // announce the conductor
    ConductorIdIs(PeerId),
    /// recv-ed the conductor id
    RecvdConductor,
}

impl NormalMessage {
    /// convert a normal message into a message to relay to conductor
    pub fn normal_to_relayed(&self) -> RelayedMessage {
        let recv_ts = SystemTime::now();
        let elapsed_time = recv_ts
            .duration_since(self.sent_ts)
            .unwrap_or(Duration::MAX);
        RelayedMessage {
            duration: elapsed_time,
            req: self.req.clone(),
            epoch: self.epoch,
        }
    }
}

/// ways to send messages between nodes
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum ConductorMessageMethod {
    /// broadcast message to all nodes
    Broadcast,
    /// direct message [`PeerId`]
    DirectMessage(PeerId),
}

/// handler for non-conductor nodes for normal messages
pub async fn handle_normal_msg(
    handle: Arc<NetworkNodeHandle<CounterState>>,
    msg: NormalMessage,
    // in case we need to reply to direct message
    chan: Option<ResponseChannel<DirectMessageResponse>>,
) -> Result<(), NetworkNodeHandleError> {
    // send reply logic
    match msg.req {
        // direct message only
        CounterRequest::StateResponse(c) => {
            handle
                .modify_state(|s| {
                    if c >= *s {
                        *s = c
                    }
                })
                .await;
        }
        // only as a response
        CounterRequest::StateRequest => {
            if let Some(chan) = chan {
                let state = handle.state().await;
                let response = Message::Normal(NormalMessage {
                    sent_ts: SystemTime::now(),
                    relay_to_conductor: true,
                    req: CounterRequest::StateResponse(state),
                    epoch: (state, state + 1),
                    padding: vec![0; PADDING_SIZE],
                });
                handle.direct_response(chan, &response).await?;
            } else {
                error!("Error deserializing, channel closed!");
            }
        }
        CounterRequest::Kill => {
            handle.shutdown().await?;
        }
    }
    // relay the message to conductor
    if msg.relay_to_conductor {
        if let Some(conductor_id) = handle.ignored_peers().await.iter().next() {
            let relayed_msg = Message::Relayed(msg.normal_to_relayed());
            handle.direct_request(*conductor_id, &relayed_msg).await?;
        } else {
            error!("We have a message to send to the conductor, but we do not know who the conductor is!");
        }
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
                info!("msg recved: {:?}", msg.clone());
                match msg {
                    Message::ConductorIdIs(peerid) => {
                        handle
                            .ignore_peers(vec![peerid])
                            .await?;
                        handle.direct_request(peerid, &Message::RecvdConductor).await?;
                    }
                    Message::Normal(msg) => {
                        handle_normal_msg(handle.clone(), msg, None).await?;
                    }
                    // do nothing. We only expect to be reached out to by the conductor via
                    // direct message
                    Message::Conductor(..) /* only the conductor expects to receive a relayed message */ | Message::Relayed(..) => { }
                    // only sent to conductor node
                    Message::RecvdConductor => {
                        unreachable!();
                    }
                }
            } else {
                info!("FAILED TO PARSE GOSSIP OR DIRECT RESPONSE MESSAGE");
            }
        }
        DirectRequest(msg, _peer_id, chan) => {
            if let Ok(msg) = deserialize_msg::<Message>(&msg) {
                info!("from pid {:?} msg recved: {:?}", msg.clone(), _peer_id);
                match msg {
                    // this is only done via broadcast
                    Message::ConductorIdIs(_)
                        // these are only sent to the conductor
                        | Message::Relayed(_) | Message::RecvdConductor =>
                    {
                        unreachable!()
                    }
                    Message::Normal(msg) => {
                        handle_normal_msg(handle.clone(), msg, Some(chan)).await?;
                    }
                    Message::Conductor(msg) => {
                        let state = handle.state().await;
                        let response =
                            Message::Normal(NormalMessage {
                                sent_ts: SystemTime::now(),
                                relay_to_conductor: true,
                                req: msg.req,
                                epoch: (state, state+1),
                                padding: vec![0; PADDING_SIZE]
                        });
                        match msg.broadcast_type {
                            // if the conductor says to broadcast
                            // perform broadcast with gossip protocol
                            ConductorMessageMethod::Broadcast => {
                                handle.gossip("global".to_string(), &response).await?;
                            }
                            ConductorMessageMethod::DirectMessage(pid) => {
                                handle.direct_request(
                                    pid,
                                    &response
                                ).await?;
                            }
                        }
                    }
                }
            } else {
            }
        }
    }
    Ok(())
}

/// convert node string into multi addr
pub fn parse_node(s: &str) -> Result<Multiaddr, multiaddr::Error> {
    let mut i = s.split(':');
    let ip = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    let port = i.next().ok_or(multiaddr::Error::InvalidMultiaddr)?;
    Multiaddr::from_str(&format!("/ip4/{}/tcp/{}", ip, port))
}

#[derive(StructOpt)]
pub struct CliOpt {
    /// list of bootstrap node addrs
    #[structopt(long = "bootstrap")]
    #[structopt(parse(try_from_str = parse_node))]
    pub bootstrap_addrs: Vec<Multiaddr>,
    /// total number of nodes
    #[structopt(long = "num_nodes")]
    pub num_nodes: usize,
    /// the role this node plays
    #[structopt(long = "node_type")]
    pub node_type: NetworkNodeType,
    /// internal interface to bind to
    #[structopt(long = "bound_addr")]
    #[structopt(parse(try_from_str = parse_node))]
    pub bound_addr: Multiaddr,
    /// If this value is set, a webserver will be spawned on this address with debug info
    #[cfg(feature = "webui")]
    #[structopt(long = "webui")]
    pub webui_addr: Option<SocketAddr>,
    /// type of environment
    #[cfg(all(feature = "lossy_network", target_os = "linux"))]
    #[structopt(long = "env")]
    pub env_type: ExecutionEnvironment,
}

/// The execution environemnt type
#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[cfg(all(feature = "lossy_network", target_os = "linux"))]
pub enum ExecutionEnvironment {
    /// execution environment is within docker
    Docker,
    /// execution environment is on metal
    Metal,
}

#[cfg(all(feature = "lossy_network", target_os = "linux"))]
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
    setup_logging();
    setup_backtrace();
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

            let known_peers = handle.known_peers().await;
            handle
                .modify_state(|s| {
                    for a_peer in &known_peers {
                        if *a_peer != handle.peer_id()
                            && s.current_epoch.node_states.get(a_peer).is_none()
                        {
                            s.current_epoch.node_states.insert(*a_peer, 0);
                        }
                    }
                })
                .await;
            handle.notify_webui().await;

            let conductor_peerid = handle.peer_id();

            let mut res_fut = handle.state_wait_timeout_until_with_trigger(TIMEOUT, |state| {
                state.ready_set.len() >= opts.num_nodes - 1
            });

            // is ready
            res_fut.next().await.unwrap().unwrap();

            let (s, r) = flume::bounded::<bool>(1);

            spawn({
                let handle = handle.clone();
                // the "conductor id"
                // periodically say "ignore me!"
                async move {
                    // must wait for the listener to start
                    let msg = Message::ConductorIdIs(conductor_peerid);
                    while r.is_empty() {
                        handle
                            .gossip("global".to_string(), &msg)
                            .await
                            .context(HandleSnafu)?;
                        sleep(Duration::from_secs(1)).await;
                    }
                    Ok::<(), CounterError>(())
                }
            });

            if res_fut.next().await.unwrap().is_err() {
                panic!("timeout waiting for conductor peerid to propagate!");
            }

            // kill conductor id broadcast thread
            s.send_async(true).await.unwrap();

            for i in 0..5 {
                handle
                    .modify_state(|s| s.current_epoch.epoch_type = EpochType::BroadcastViaGossip)
                    .await;
                conductor_broadcast(TIMEOUT, i, handle.clone())
                    .await
                    .context(HandleSnafu)?;
                handle
                    .modify_state(|s| s.complete_round(EpochType::BroadcastViaGossip))
                    .await;
            }

            for j in 5..10 {
                handle
                    .modify_state(|s| s.current_epoch.epoch_type = EpochType::DMViaDM)
                    .await;
                conductor_direct_message(TIMEOUT, j, handle.clone())
                    .await
                    .context(HandleSnafu)?;
                handle
                    .modify_state(|s| s.complete_round(EpochType::DMViaDM))
                    .await;
            }

            let kill_msg = Message::Normal(NormalMessage {
                req: CounterRequest::Kill,
                relay_to_conductor: false,
                sent_ts: SystemTime::now(),
                epoch: (10, 11),
                padding: vec![0; PADDING_SIZE],
            });

            for peer_id in handle.connected_peers().await {
                handle
                    .direct_request(peer_id, &kill_msg)
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

            let node = NetworkNodeHandle::<CounterState>::new(config.clone(), 0)
                .await
                .context(HandleSnafu)?;

            let handle = Arc::new(node);
            #[cfg(feature = "webui")]
            if let Some(addr) = opts.webui_addr {
                web::spawn_server(Arc::clone(&handle), addr);
            }

            spin_up_swarm(TIMEOUT, bootstrap_nodes, config, 0, &handle)
                .await
                .context(HandleSnafu)?;
            spawn_handler(handle.clone(), regular_handle_network_event).await;
            while !handle.is_killed().await {
                async_std::task::sleep(Duration::from_millis(100)).await;
            }
        }
    }

    Ok(())
}

/// have conductor direct message all participants
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
    let chosen_peer = *known_peers.iter().choose(&mut thread_rng()).unwrap();

    // step 1: increment counter on the chosen/"leader" node

    let handle = handle.clone();

    // set up listener before any state has the chance to change
    let mut res_fut = handle.state_wait_timeout_until_with_trigger(timeout, |state| {
        *state.current_epoch.node_states.get(&chosen_peer).unwrap() >= new_state
    });

    res_fut.next().await.unwrap().unwrap();

    // dispatch message
    let msg = Message::Normal(NormalMessage {
        sent_ts: SystemTime::now(),
        relay_to_conductor: true,
        req: CounterRequest::StateResponse(handle.state().await.current_epoch.epoch_idx.1),
        epoch: handle.state().await.current_epoch.epoch_idx,
        padding: vec![0; PADDING_SIZE],
    });
    handle.direct_request(chosen_peer, &msg).await?;

    if res_fut.next().await.unwrap().is_err() {
        panic!("failed to send!");
    }

    // step 2: iterate through remaining nodes, message them "request state from chosen node"

    let res_fut = handle.state_wait_timeout_until(timeout, |state| {
        state
            .current_epoch
            .node_states
            .iter()
            .all(|(_k, &s)| s >= new_state)
    });

    // send out the requests to ask the chosen peer for its state (and replace ours)

    let mut remaining_nodes = known_peers.clone();
    remaining_nodes.remove(&chosen_peer);

    for peer in &remaining_nodes {
        let msg_increment = Message::Conductor(ConductorMessage {
            req: CounterRequest::StateRequest,
            broadcast_type: ConductorMessageMethod::DirectMessage(chosen_peer),
        });
        handle.direct_request(*peer, &msg_increment).await?;
    }

    if res_fut.await.is_err() {
        panic!("failed to send!");
    }

    Ok(())
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
    let chosen_peer = *known_peers.iter().choose(&mut thread_rng()).unwrap();

    let request = CounterRequest::StateResponse(handle.state().await.current_epoch.epoch_idx.1);

    // tell the "leader" to do a "broadcast" message using gosisp protocol
    let msg = Message::Conductor(ConductorMessage {
        req: request.clone(),
        broadcast_type: ConductorMessageMethod::Broadcast,
    });

    let mut res_fut = handle.state_wait_timeout_until_with_trigger(timeout, |state| {
        state
            .current_epoch
            .node_states
            .iter()
            .all(|(_, &s)| s >= new_state)
    });

    // wait for ready signal
    res_fut.next().await.unwrap().unwrap();

    // always spawn listener FIRST
    let increment_leader_msg = Message::Normal(NormalMessage {
        sent_ts: SystemTime::now(),
        relay_to_conductor: true,
        req: CounterRequest::StateResponse(handle.state().await.current_epoch.epoch_idx.1),
        epoch: handle.state().await.current_epoch.epoch_idx,
        padding: vec![0; PADDING_SIZE],
    });
    // send direct message from conductor to leader to do broadcast
    handle
        .direct_request(chosen_peer, &msg)
        .await
        .context(HandleSnafu)
        .unwrap();

    handle
        .direct_request(chosen_peer, &increment_leader_msg)
        .await
        .context(HandleSnafu)
        .unwrap();

    if res_fut.next().await.unwrap().is_err() {
        panic!("timeout!");
    }

    Ok(())
}

/// network event handler for conductor
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
            // it's only purpose is to recv relayed messages
        }
        DirectRequest(m, peer_id, _chan) => {
            if let Ok(msg) = deserialize_msg::<Message>(&m) {
                match msg {
                    Message::Relayed(msg) => {
                        match handle.state().await.current_epoch.epoch_type {
                            EpochType::BroadcastViaGossip => {
                                // FIXME should check epoch
                                if let CounterRequest::StateResponse(..) = msg.req {
                                    handle
                                        .modify_state(|s| {
                                            s.current_epoch.message_durations.push(msg.duration)
                                        })
                                        .await;
                                }
                            }
                            EpochType::DMViaDM => {
                                // FIXME should check epoch
                                if let CounterRequest::StateRequest = msg.req {
                                    handle
                                        .modify_state(|s| {
                                            s.current_epoch.message_durations.push(msg.duration)
                                        })
                                        .await;
                                }
                            }
                            EpochType::BroadcastViaDM => {
                                unimplemented!("BroadcastViaDM is currently unimplemented");
                            }
                        }
                        if let CounterRequest::StateResponse(state) = msg.req {
                            handle
                                .modify_state(|s| {
                                    if let Some(rec_state) =
                                        s.current_epoch.node_states.get(&peer_id)
                                    {
                                        if *rec_state < state {
                                            s.current_epoch.node_states.insert(peer_id, state);
                                        }
                                    } else {
                                        s.current_epoch.node_states.insert(peer_id, state);
                                    }
                                })
                                .await;
                        }
                    }
                    Message::RecvdConductor => {
                        handle
                            .modify_state(|s| {
                                s.ready_set.insert(peer_id);
                            })
                            .await;
                    }
                    msg => {
                        info!("Unexpected message {:?}", msg);

                        /* Do nothing. Conductor doesn't care about these messages. */
                    }
                }
            } else {
                error!("failed to deserialize msg");
            }
        }
        DirectResponse(_m, _peer_id) => { /* nothing to do here */ }
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
