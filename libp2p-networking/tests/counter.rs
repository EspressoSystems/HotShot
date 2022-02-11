use std::{sync::Arc, time::Duration};
mod common;
use async_std::future::timeout;
use common::{check_connection_state, test_bed, HandleSnafu, TestError};

use bincode::Options;

use futures::future::join_all;
use libp2p::gossipsub::Topic;
use networking_demo::{
    network_node::{ClientRequest, NetworkEvent},
    network_node_handle::{
        get_random_handle, NetworkNodeHandle, NetworkNodeHandleError, SendSnafu, SerializationSnafu,
    },
};
use std::fmt::Debug;

use serde::{Deserialize, Serialize};

use snafu::ResultExt;

use tracing::{info, instrument, warn};

use crate::common::print_connections;

pub type CounterState = u32;

const TOTAL_NUM_PEERS: usize = 30;
const NUM_OF_BOOTSTRAP: usize = 5;
const TIMEOUT: Duration = Duration::from_secs(200);
const NUM_ROUNDS: usize = 100;

/// Message types. We can either
/// - increment the Counter
/// - request a counter value
/// - reply with a counter value
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum CounterMessage {
    IncrementCounter {
        from: CounterState,
        to: CounterState,
    },
    AskForCounter,
    MyCounterIs(CounterState),
}

/// event handler for events from the swarm
/// - updates state based on events received
/// - replies to direct messages
#[instrument]
pub async fn counter_handle_network_event(
    event: NetworkEvent,
    handle: Arc<NetworkNodeHandle<CounterState>>,
) -> Result<(), NetworkNodeHandleError> {
    use CounterMessage::*;
    #[allow(clippy::enum_glob_use)]
    use NetworkEvent::*;
    let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
    match event {
        GossipMsg(m) | DirectResponse(m) => {
            if let Ok(msg) = bincode_options.deserialize::<CounterMessage>(&m) {
                match msg {
                    MyCounterIs(c) => {
                        *handle.state.lock().await = c;
                        handle.state_changed.notify_all();
                    }
                    IncrementCounter { from, to, .. } => {
                        if *handle.state.lock().await == from {
                            *handle.state.lock().await = to;
                            handle.state_changed.notify_all();
                        }
                    }
                    AskForCounter => {}
                }
            }
        }
        DirectRequest(m, chan) => {
            if let Ok(msg) = bincode_options.deserialize::<CounterMessage>(&m) {
                match msg {
                    IncrementCounter { from, to, .. } => {
                        if *handle.state.lock().await == from {
                            *handle.state.lock().await = to;
                            handle.state_changed.notify_all();
                        }
                    }
                    AskForCounter => {
                        let response = MyCounterIs(*handle.state.lock().await);
                        let serialized_response = bincode_options
                            .serialize(&response)
                            .context(SerializationSnafu)?;
                        handle
                            .send_network
                            .send_async(ClientRequest::DirectResponse(chan, serialized_response))
                            .await
                            .context(SendSnafu)?
                    }
                    MyCounterIs(_) => {}
                }
            }
        }
        UpdateConnectedPeers(p) => {
            handle.connection_state.lock().await.connected_peers = p;
        }
        UpdateKnownPeers(p) => {
            handle.connection_state.lock().await.known_peers = p;
        }
    };
    Ok(())
}

/// `requester_handle` asks for `requestee_handle`'s state,
/// and then `requester_handle` updates its state to equal `requestee_handle`.
async fn run_request_response_increment(
    requester_handle: Arc<NetworkNodeHandle<CounterState>>,
    requestee_handle: Arc<NetworkNodeHandle<CounterState>>,
) -> Result<(), TestError<CounterState>> {
    let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
    let msg_inner = bincode_options
        .serialize(&CounterMessage::AskForCounter)
        .unwrap();

    let msg = ClientRequest::DirectRequest(requestee_handle.peer_id, msg_inner);

    let new_state = *requestee_handle.state.lock().await;

    // set up state change listener
    let recv_fut = requester_handle.state_changed.wait_timeout_until(
        requester_handle.state.lock().await,
        TIMEOUT,
        |state| *state == new_state,
    );

    requester_handle
        .send_network
        .send_async(msg)
        .await
        .context(SendSnafu)
        .context(HandleSnafu)?;

    let (_, res) = recv_fut.await;

    if res.timed_out() {
        Err(TestError::DirectTimeout {
            requester: requester_handle.id,
            requestee: requestee_handle.id,
        })
    } else {
        let s1 = *requester_handle.state.lock().await;
        // sanity check
        if s1 != new_state {
            Err(TestError::State {
                id: requester_handle.id,
                expected: new_state,
                actual: s1,
            })
        } else {
            Ok(())
        }
    }
}

/// broadcasts `msg` from a randomly chosen handle
/// then asserts that all nodes match `new_state`
async fn run_gossip_round(
    handles: &[Arc<NetworkNodeHandle<CounterState>>],
    msg_inner: Vec<u8>,
    new_state: CounterState,
) -> Result<(), TestError<CounterState>> {
    let msg_handle = get_random_handle(handles);
    *msg_handle.state.lock().await = new_state;

    let mut futs = Vec::new();
    for handle in handles {
        let a_fut = handle
            .state_changed
            .wait_until(handle.state.lock().await, |state| *state == new_state);
        futs.push(a_fut);
    }

    let msg = ClientRequest::GossipMsg(Topic::new("global"), msg_inner.clone());
    msg_handle
        .send_network
        .send_async(msg)
        .await
        .context(SendSnafu)
        .context(HandleSnafu)?;

    if timeout(TIMEOUT, futures::future::join_all(futs))
        .await
        .is_err()
    {
        let mut failing = Vec::new();
        for handle in handles.iter() {
            let handle_state = *handle.state.lock().await;
            if handle_state != new_state {
                failing.push(handle.id);
            }
        }
        if !failing.is_empty() {
            print_connections(handles).await;
            return Err(TestError::GossipTimeout { failing });
        }
    }
    Ok(())
}

/// runs `num_rounds` of message broadcast, incrementing the state of all nodes each broadcast
async fn run_gossip_rounds(
    handles: &[Arc<NetworkNodeHandle<CounterState>>],
    num_rounds: usize,
    starting_state: CounterState,
) {
    let mut old_state = starting_state;
    let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
    for i in 0..num_rounds {
        info!("running gossip round {}", i);
        let new_state = old_state + 1;
        let msg_inner = bincode_options
            .serialize(&CounterMessage::IncrementCounter {
                from: old_state,
                to: new_state,
            })
            .unwrap();
        run_gossip_round(handles, msg_inner, new_state)
            .await
            .unwrap();
        old_state = new_state;
    }
}

/// chooses a random handle from `handles`
/// increments its state by 1,
/// then has all other peers request its state
/// and update their state to the recv'ed state
async fn run_request_response_increment_all(handles: &[Arc<NetworkNodeHandle<CounterState>>]) {
    check_connection_state(handles).await;
    let requestee_handle = get_random_handle(handles);
    *requestee_handle.state.lock().await += 1;
    requestee_handle
        .send_network
        .send_async(ClientRequest::Pruning(false))
        .await
        .unwrap();
    let mut futs = Vec::new();
    for (_i, h) in handles.iter().enumerate() {
        // skip `requestee_handle`
        if h.peer_id != requestee_handle.peer_id {
            let requester_handle = h.clone();
            futs.push(run_request_response_increment(
                requester_handle,
                requestee_handle.clone(),
            ));
        }
    }
    let results = join_all(futs).await;
    for result in &results {
        if result.is_err() {
            print_connections(handles).await;
            panic!(
                "{:?}",
                results
                    .into_iter()
                    .filter(|r| r.is_err())
                    .collect::<Vec<_>>()
            );
        }
    }

    check_connection_state(handles).await;
    requestee_handle
        .send_network
        .send_async(ClientRequest::Pruning(true))
        .await
        .unwrap();
}

/// simple case of direct message
#[async_std::test]
#[instrument]
async fn test_request_response_one_round() {
    pub async fn run_request_response_one_round(
        handles: Vec<Arc<NetworkNodeHandle<CounterState>>>,
    ) {
        run_request_response_increment_all(&handles).await;
        for h in handles.into_iter() {
            assert_eq!(*h.state.lock().await, 1);
        }
    }
    test_bed(
        run_request_response_one_round,
        counter_handle_network_event,
        TOTAL_NUM_PEERS,
        NUM_OF_BOOTSTRAP,
        TIMEOUT,
    )
    .await
}

/// stress test of direct messsage
#[async_std::test]
#[instrument]
async fn test_request_response_many_rounds() {
    pub async fn run_request_response_many_rounds(
        handles: Vec<Arc<NetworkNodeHandle<CounterState>>>,
    ) {
        let num_rounds = 4092;
        for _i in 0..num_rounds {
            run_request_response_increment_all(&handles).await;
        }
        for h in handles.into_iter() {
            assert_eq!(*h.state.lock().await, num_rounds);
        }
    }
    test_bed(
        run_request_response_many_rounds,
        counter_handle_network_event,
        TOTAL_NUM_PEERS,
        NUM_OF_BOOTSTRAP,
        TIMEOUT,
    )
    .await
}

/// stress test of broadcast + direct message
#[async_std::test]
#[instrument]
async fn test_intersperse_many_rounds() {
    pub async fn run_intersperse_many_rounds(handles: Vec<Arc<NetworkNodeHandle<CounterState>>>) {
        for i in 0..NUM_ROUNDS as u32 {
            if i % 2 == 0 {
                run_request_response_increment_all(&handles).await;
            } else {
                run_gossip_rounds(&handles, 1, i).await
            }
            // println!("finished {}", i);
        }
        for h in handles.into_iter() {
            assert_eq!(*h.state.lock().await, NUM_ROUNDS as u32);
        }
    }
    test_bed(
        run_intersperse_many_rounds,
        counter_handle_network_event,
        TOTAL_NUM_PEERS,
        NUM_OF_BOOTSTRAP,
        TIMEOUT,
    )
    .await
}

/// stress teset that we can broadcast a message out and get counter increments
#[async_std::test]
#[instrument]
async fn test_gossip_many_rounds() {
    pub async fn run_gossip_many_rounds(handles: Vec<Arc<NetworkNodeHandle<CounterState>>>) {
        run_gossip_rounds(&handles, NUM_ROUNDS, 0).await
    }
    test_bed(
        run_gossip_many_rounds,
        counter_handle_network_event,
        TOTAL_NUM_PEERS,
        NUM_OF_BOOTSTRAP,
        TIMEOUT,
    )
    .await;
}

/// simple case of broadcast message
#[async_std::test]
#[instrument]
async fn test_gossip_one_round() {
    pub async fn run_gossip_one_round(handles: Vec<Arc<NetworkNodeHandle<CounterState>>>) {
        run_gossip_rounds(&handles, 1, 0).await
    }
    test_bed(
        run_gossip_one_round,
        counter_handle_network_event,
        TOTAL_NUM_PEERS,
        NUM_OF_BOOTSTRAP,
        TIMEOUT,
    )
    .await;
}
