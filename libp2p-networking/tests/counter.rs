mod common;

use crate::common::print_connections;
use async_std::prelude::StreamExt;
use bincode::Options;
use common::{test_bed, HandleSnafu, TestError};
use futures::future::join_all;
use libp2p_networking::network::{
    get_random_handle, NetworkEvent, NetworkNodeHandle, NetworkNodeHandleError,
};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::{fmt::Debug, sync::Arc, time::Duration};
use tracing::{error, info, instrument, warn};

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
                    }
                    // only as a response
                    AskForCounter => {}
                }
            } else {
                error!("FAILED TO DESERIALIZE MSG {:?}", m);
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
                    }
                    // direct message response
                    AskForCounter => {
                        let response = MyCounterIs(handle.state().await);
                        handle.direct_response(chan, &response).await?;
                    }
                    MyCounterIs(_) => {}
                }
            }
        }
    };
    Ok(())
}

/// `requester_handle` asks for `requestee_handle`'s state,
/// and then `requester_handle` updates its state to equal `requestee_handle`.
async fn run_request_response_increment<'a>(
    requester_handle: Arc<NetworkNodeHandle<CounterState>>,
    requestee_handle: Arc<NetworkNodeHandle<CounterState>>,
    timeout: Duration,
) -> Result<(), TestError<CounterState>> {
    async move {
        let new_state = requestee_handle.state().await;

        // set up state change listener
        let mut stream = requester_handle
            .state_wait_timeout_until_with_trigger(timeout, move |state| *state == new_state);

        let requestee_pid = requestee_handle.peer_id();

        // TODO fix error handling
        stream.next().await.unwrap().unwrap();
        requester_handle
            .direct_request(requestee_pid, &CounterMessage::AskForCounter)
            .await
            .context(HandleSnafu)?;
        stream.next().await.unwrap().unwrap();

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
    .await
}

/// broadcasts `msg` from a randomly chosen handle
/// then asserts that all nodes match `new_state`
async fn run_gossip_round(
    handles: &[Arc<NetworkNodeHandle<CounterState>>],
    msg: CounterMessage,
    new_state: CounterState,
    timeout_duration: Duration,
) -> Result<(), TestError<CounterState>> {
    let msg_handle = get_random_handle(handles);
    msg_handle.modify_state(|s| *s = new_state).await;

    let mut futs = Vec::new();
    let len = handles.len();
    for handle in handles {
        // already modified, so skip msg_handle
        if handle.peer_id() != msg_handle.peer_id() {
            let stream = handle.state_wait_timeout_until_with_trigger(timeout_duration, |state| {
                *state == new_state
            });
            futs.push(stream);
        }
    }

    let mut merged_streams = futures::stream::select_all(futs);

    // make sure all are ready/listening
    for i in 0..len - 1 {
        // unwrap is okay because stream must have 2 * (len - 1) elements
        match merged_streams.next().await.unwrap() {
            Ok(()) => {}
            Err(e) => panic!(
                "timeout : {:?} waiting handle {:?} to subscribe to state events",
                e, i
            ),
        }
    }

    msg_handle
        .gossip("global".to_string(), &msg)
        .await
        .context(HandleSnafu)?;

    for _ in 0..len - 1 {
        // wait for all events to finish
        // then check for failures
        let _ = merged_streams.next().await;
    }

    let mut failing = Vec::new();
    for handle in handles.iter() {
        let handle_state = handle.state().await;
        if handle_state != new_state {
            failing.push(handle.id());
            println!("state: {:?}, expected: {:?}", handle_state, new_state);
        }
    }
    if !failing.is_empty() {
        print_connections(handles).await;
        return Err(TestError::GossipTimeout { failing });
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
        error!("round: {:?}", i);
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
    for i in 0..num_rounds {
        info!("running gossip round {}", i);
        let new_state = old_state + 1;
        let msg = CounterMessage::IncrementCounter {
            from: old_state,
            to: new_state,
        };
        run_gossip_round(handles, msg, new_state, timeout)
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
    requestee_handle.toggle_prune(false).await.unwrap();
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
    if results.iter().any(|x| x.is_err()) {
        print_connections(handles).await;
        panic!(
            "{:?}",
            results
                .into_iter()
                .filter(|r| r.is_err())
                .collect::<Vec<_>>()
        );
    }

    requestee_handle.toggle_prune(true).await.unwrap();
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

// TODO(https://github.com/EspressoSystems/phaselock/issues/220): enable this
// /// simple case of direct message
// #[async_std::test]
// #[instrument]
// #[ignore]
// async fn test_stress_request_response_one_round() {
//     test_bed(
//         run_request_response_one_round,
//         counter_handle_network_event,
//         TOTAL_NUM_PEERS_STRESS,
//         NUM_OF_BOOTSTRAP_STRESS,
//         TIMEOUT_STRESS,
//     )
//     .await
// }

// TODO(https://github.com/EspressoSystems/phaselock/issues/220): enable this
// /// stress test of direct messsage
// #[async_std::test]
// #[instrument]
// #[ignore]
// async fn test_stress_request_response_many_rounds() {
//     test_bed(
//         run_request_response_many_rounds,
//         counter_handle_network_event,
//         TOTAL_NUM_PEERS_STRESS,
//         NUM_OF_BOOTSTRAP_STRESS,
//         TIMEOUT_STRESS,
//     )
//     .await
// }

// TODO(https://github.com/EspressoSystems/phaselock/issues/220): enable this
// /// stress test of broadcast + direct message
// #[async_std::test]
// #[instrument]
// #[ignore]
// async fn test_stress_intersperse_many_rounds() {
//     test_bed(
//         run_intersperse_many_rounds,
//         counter_handle_network_event,
//         TOTAL_NUM_PEERS_STRESS,
//         NUM_OF_BOOTSTRAP_STRESS,
//         TIMEOUT_STRESS,
//     )
//     .await
// }

/// stress teset that we can broadcast a message out and get counter increments
#[async_std::test]
#[instrument]
#[ignore]
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
#[ignore]
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
#[ignore]
async fn test_stress_dht_one_round() {
    test_bed(
        run_dht_one_round,
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
#[ignore]
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
