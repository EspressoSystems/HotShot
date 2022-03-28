mod common;

use crate::common::print_connections;
use async_std::{future::timeout, task::spawn};
use bincode::Options;
use common::{test_bed, HandleSnafu, TestError};
use futures::future::join_all;
use libp2p::gossipsub::Topic;
use networking_demo::network::{
    get_random_handle, network_node_handle_error::SerializationSnafu, ClientRequest, NetworkEvent,
    NetworkNodeHandle, NetworkNodeHandleError,
};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::{fmt::Debug, sync::Arc, time::Duration};
use tracing::{info, instrument, warn};

pub type CounterState = u32;

const NUM_ROUNDS: usize = 100;

const TOTAL_NUM_PEERS_COVERAGE: usize = 8;
const NUM_OF_BOOTSTRAP_COVERAGE: usize = 3;
const TIMEOUT_COVERAGE: Duration = Duration::from_secs(120);

const TOTAL_NUM_PEERS_STRESS: usize = 15;
const NUM_OF_BOOTSTRAP_STRESS: usize = 3;
const TIMEOUT_STRESS: Duration = Duration::from_secs(60);

const DHT_KV_PADDING: usize = 1024;

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
    #[allow(clippy::enum_glob_use)]
    use CounterMessage::*;
    use NetworkEvent::*;
    let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
    match event {
        GossipMsg(m) | DirectResponse(m, _) => {
            if let Ok(msg) = bincode_options.deserialize::<CounterMessage>(&m) {
                match msg {
                    // direct message only
                    MyCounterIs(c) => {
                        handle.modify_state(|s| *s = c).await;
                    }
                    // gossip message only
                    IncrementCounter { from, to, .. } => {
                        handle
                            .modify_state(|s| {
                                if *s == from {
                                    *s = to;
                                }
                            })
                            .await;
                        // if *handle.state.lock().await == from {
                        //     *handle.state.lock().await = to;
                        //     handle.state_changed.notify_all();
                        //     handle.notify_webui().await;
                        // }
                    }
                    // only as a response
                    AskForCounter => {}
                }
            }
        }
        DirectRequest(m, _, chan) => {
            if let Ok(msg) = bincode_options.deserialize::<CounterMessage>(&m) {
                match msg {
                    // direct message request
                    IncrementCounter { from, to, .. } => {
                        handle
                            .modify_state(|s| {
                                if *s == from {
                                    *s = to
                                }
                            })
                            .await;
                        // if *handle.state.lock().await == from {
                        //     *handle.state.lock().await = to;
                        //     handle.state_changed.notify_all();
                        //     handle.notify_webui().await;
                        // }
                    }
                    // direct message response
                    AskForCounter => {
                        let response = MyCounterIs(handle.state().await);
                        let serialized_response = bincode_options
                            .serialize(&response)
                            .context(SerializationSnafu)?;
                        handle
                            .send_request(ClientRequest::DirectResponse(chan, serialized_response))
                            .await?;
                    }
                    MyCounterIs(_) => {}
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
    };
    Ok(())
}

/// `requester_handle` asks for `requestee_handle`'s state,
/// and then `requester_handle` updates its state to equal `requestee_handle`.
async fn run_request_response_increment(
    requester_handle: Arc<NetworkNodeHandle<CounterState>>,
    requestee_handle: Arc<NetworkNodeHandle<CounterState>>,
    timeout: Duration,
) -> Result<(), TestError<CounterState>> {
    let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
    let msg_inner = bincode_options
        .serialize(&CounterMessage::AskForCounter)
        .unwrap();

    let msg = ClientRequest::DirectRequest(requestee_handle.peer_id(), msg_inner);

    let new_state = requestee_handle.state().await;

    // set up state change listener
    let recv_fut = requester_handle.state_wait_timeout_until(timeout, |state| *state == new_state);

    let requester_handle_dup = requester_handle.clone();

    spawn(async move {
        requester_handle_dup
            .send_request(msg)
            .await
            .context(HandleSnafu)?;
        Ok::<(), TestError<CounterState>>(())
    });

    let res = recv_fut.await;

    if res.is_err() {
        Err(TestError::DirectTimeout {
            requester: requester_handle.id(),
            requestee: requestee_handle.id(),
        })
    } else {
        let s1 = requester_handle.state().await;
        // sanity check
        if s1 != new_state {
            Err(TestError::State {
                id: requester_handle.id(),
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
    timeout_duration: Duration,
) -> Result<(), TestError<CounterState>> {
    let msg_handle = get_random_handle(handles);
    msg_handle.modify_state(|s| *s = new_state).await;

    let mut futs = Vec::new();
    for handle in handles {
        // already modified, so skip msg_handle
        if handle.peer_id() != msg_handle.peer_id() {
            let a_fut =
                handle.state_wait_timeout_until(timeout_duration, |state| *state == new_state);
            futs.push(a_fut);
        }
    }

    spawn(async move {
        let msg = ClientRequest::GossipMsg(Topic::new("global"), msg_inner.clone());
        msg_handle.send_request(msg).await.context(HandleSnafu)?;
        Ok::<(), TestError<CounterState>>(())
    });

    // must launch listener futures BEFORE sending increment message

    if timeout(timeout_duration, futures::future::join_all(futs))
        .await
        .is_err()
    {
        let mut failing = Vec::new();
        for handle in handles.iter() {
            let handle_state = handle.state().await;
            if handle_state != new_state {
                failing.push(handle.id());
            }
        }
        if !failing.is_empty() {
            print_connections(handles).await;
            return Err(TestError::GossipTimeout { failing });
        }
    }
    Ok(())
}

async fn run_intersperse_many_rounds(
    handles: Vec<Arc<NetworkNodeHandle<CounterState>>>,
    timeout: Duration,
) {
    for i in 0..NUM_ROUNDS as u32 {
        if i % 2 == 0 {
            run_request_response_increment_all(&handles, timeout).await;
        } else {
            run_gossip_rounds(&handles, 1, i, timeout).await
        }
        // println!("finished {}", i);
    }
    for h in handles.into_iter() {
        assert_eq!(h.state().await, NUM_ROUNDS as u32);
    }
}

async fn run_dht_many_rounds(
    handles: Vec<Arc<NetworkNodeHandle<CounterState>>>,
    timeout: Duration,
) {
    run_dht_rounds(&handles, timeout, 0, NUM_ROUNDS).await;
}

async fn run_dht_one_round(handles: Vec<Arc<NetworkNodeHandle<CounterState>>>, timeout: Duration) {
    run_dht_rounds(&handles, timeout, 0, 1).await;
}

async fn run_request_response_many_rounds(
    handles: Vec<Arc<NetworkNodeHandle<CounterState>>>,
    timeout: Duration,
) {
    for _i in 0..NUM_ROUNDS {
        run_request_response_increment_all(&handles, timeout).await;
    }
    for h in handles.into_iter() {
        assert_eq!(h.state().await, NUM_ROUNDS as u32);
    }
}

pub async fn run_request_response_one_round(
    handles: Vec<Arc<NetworkNodeHandle<CounterState>>>,
    timeout: Duration,
) {
    run_request_response_increment_all(&handles, timeout).await;
    for h in handles.into_iter() {
        assert_eq!(h.state().await, 1);
    }
}

pub async fn run_gossip_many_rounds(
    handles: Vec<Arc<NetworkNodeHandle<CounterState>>>,
    timeout: Duration,
) {
    run_gossip_rounds(&handles, NUM_ROUNDS, 0, timeout).await
}

async fn run_gossip_one_round(
    handles: Vec<Arc<NetworkNodeHandle<CounterState>>>,
    timeout: Duration,
) {
    run_gossip_rounds(&handles, 1, 0, timeout).await
}

async fn run_dht_rounds(
    handles: &[Arc<NetworkNodeHandle<CounterState>>],
    timeout: Duration,
    starting_val: usize,
    num_rounds: usize,
) {
    for i in 0..num_rounds {
        let msg_handle = get_random_handle(handles);
        let mut key = vec![0; DHT_KV_PADDING];
        key.push((starting_val + i) as u8);
        let mut value = vec![0; DHT_KV_PADDING];
        value.push((starting_val + i) as u8);

        // put the key
        msg_handle.put_record(&key, &value).await.unwrap();

        // get the key from the other nodes
        for handle in handles.iter() {
            let result: Result<Vec<u8>, NetworkNodeHandleError> =
                handle.get_record_timeout(&key, timeout).await;
            match result {
                Err(e) => {
                    panic!("DHT error {:?} during GET", e);
                }
                Ok(v) => {
                    assert_eq!(v, value);
                    break;
                }
            }
        }
    }
}

/// runs `num_rounds` of message broadcast, incrementing the state of all nodes each broadcast
async fn run_gossip_rounds(
    handles: &[Arc<NetworkNodeHandle<CounterState>>],
    num_rounds: usize,
    starting_state: CounterState,
    timeout: Duration,
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
        run_gossip_round(handles, msg_inner, new_state, timeout)
            .await
            .unwrap();
        old_state = new_state;
    }
}

/// chooses a random handle from `handles`
/// increments its state by 1,
/// then has all other peers request its state
/// and update their state to the recv'ed state
async fn run_request_response_increment_all(
    handles: &[Arc<NetworkNodeHandle<CounterState>>],
    timeout: Duration,
) {
    let requestee_handle = get_random_handle(handles);
    requestee_handle.modify_state(|s| *s += 1).await;
    requestee_handle
        .send_request(ClientRequest::Pruning(false))
        .await
        .unwrap();
    let mut futs = Vec::new();
    for (_i, h) in handles.iter().enumerate() {
        // skip `requestee_handle`
        if h.peer_id() != requestee_handle.peer_id() {
            let requester_handle = h.clone();
            futs.push(run_request_response_increment(
                requester_handle,
                requestee_handle.clone(),
                timeout,
            ));
        }
    }
    let results = join_all(futs).await;
    if results.iter().find(|x| x.is_err()).is_some() {
        print_connections(handles).await;
        panic!(
            "{:?}",
            results
                .into_iter()
                .filter(|r| r.is_err())
                .collect::<Vec<_>>()
        );
    }

    requestee_handle
        .send_request(ClientRequest::Pruning(true))
        .await
        .unwrap();
}

/// simple case of direct message
#[async_std::test]
#[instrument]
async fn test_coverage_request_response_one_round() {
    test_bed(
        run_request_response_one_round,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        NUM_OF_BOOTSTRAP_COVERAGE,
        TIMEOUT_COVERAGE,
    )
    .await
}

/// stress test of direct messsage
#[async_std::test]
#[instrument]
async fn test_coverage_request_response_many_rounds() {
    test_bed(
        run_request_response_many_rounds,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        NUM_OF_BOOTSTRAP_COVERAGE,
        TIMEOUT_COVERAGE,
    )
    .await
}

/// stress test of broadcast + direct message
#[async_std::test]
#[instrument]
async fn test_coverage_intersperse_many_rounds() {
    test_bed(
        run_intersperse_many_rounds,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        NUM_OF_BOOTSTRAP_COVERAGE,
        TIMEOUT_COVERAGE,
    )
    .await
}

/// stress teset that we can broadcast a message out and get counter increments
#[async_std::test]
#[instrument]
async fn test_coverage_gossip_many_rounds() {
    test_bed(
        run_gossip_many_rounds,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        NUM_OF_BOOTSTRAP_COVERAGE,
        TIMEOUT_COVERAGE,
    )
    .await;
}

/// simple case of broadcast message
#[async_std::test]
#[instrument]
async fn test_coverage_gossip_one_round() {
    test_bed(
        run_gossip_one_round,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        NUM_OF_BOOTSTRAP_COVERAGE,
        TIMEOUT_COVERAGE,
    )
    .await;
}

/// simple case of direct message
#[async_std::test]
#[instrument]
async fn test_stress_request_response_one_round() {
    test_bed(
        run_request_response_one_round,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        NUM_OF_BOOTSTRAP_STRESS,
        TIMEOUT_STRESS,
    )
    .await
}

/// stress test of direct messsage
#[async_std::test]
#[instrument]
async fn test_stress_request_response_many_rounds() {
    test_bed(
        run_request_response_many_rounds,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        NUM_OF_BOOTSTRAP_STRESS,
        TIMEOUT_STRESS,
    )
    .await
}

/// stress test of broadcast + direct message
#[async_std::test]
#[instrument]
async fn test_stress_intersperse_many_rounds() {
    test_bed(
        run_intersperse_many_rounds,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        NUM_OF_BOOTSTRAP_STRESS,
        TIMEOUT_STRESS,
    )
    .await
}

/// stress teset that we can broadcast a message out and get counter increments
#[async_std::test]
#[instrument]
async fn test_stress_gossip_many_rounds() {
    test_bed(
        run_gossip_many_rounds,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        NUM_OF_BOOTSTRAP_STRESS,
        TIMEOUT_STRESS,
    )
    .await;
}

/// simple case of broadcast message
#[async_std::test]
#[instrument]
async fn test_stress_gossip_one_round() {
    test_bed(
        run_gossip_one_round,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        NUM_OF_BOOTSTRAP_STRESS,
        TIMEOUT_STRESS,
    )
    .await;
}

/// simple case of one dht publish event
#[async_std::test]
#[instrument]
async fn test_stress_dht_one_round() {
    test_bed(
        run_gossip_one_round,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        NUM_OF_BOOTSTRAP_STRESS,
        TIMEOUT_STRESS,
    )
    .await;
}

/// many dht publishing events
#[async_std::test]
#[instrument]
async fn test_stress_dht_many_rounds() {
    test_bed(
        run_dht_many_rounds,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        NUM_OF_BOOTSTRAP_STRESS,
        TIMEOUT_STRESS,
    )
    .await;
}

/// simple case of one dht publish event
#[async_std::test]
#[instrument]
async fn test_coverage_dht_one_round() {
    test_bed(
        run_dht_one_round,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        NUM_OF_BOOTSTRAP_COVERAGE,
        TIMEOUT_COVERAGE,
    )
    .await;
}

/// many dht publishing events
#[async_std::test]
#[instrument]
async fn test_coverage_dht_many_rounds() {
    test_bed(
        run_dht_many_rounds,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        NUM_OF_BOOTSTRAP_COVERAGE,
        TIMEOUT_COVERAGE,
    )
    .await;
}
