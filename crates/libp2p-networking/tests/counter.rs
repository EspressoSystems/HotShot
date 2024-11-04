// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

#![allow(clippy::panic)]

mod common;
use std::{fmt::Debug, sync::Arc, time::Duration};

use async_lock::RwLock;
use common::{test_bed, HandleWithState, TestError};
use hotshot_example_types::node_types::TestTypes;
use hotshot_types::traits::{
    network::NetworkError, node_implementation::NodeType, signature_key::SignatureKey,
};
use libp2p_networking::network::{
    behaviours::dht::record::{Namespace, RecordKey, RecordValue},
    NetworkEvent,
};
use rand::{rngs::StdRng, seq::IteratorRandom, Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use tokio::{
    spawn,
    task::JoinSet,
    time::{sleep, Instant},
};
use tracing::{debug, error, info, instrument, warn};

use crate::common::print_connections;

pub type CounterState = u32;

const NUM_ROUNDS: usize = 100;

const TOTAL_NUM_PEERS_COVERAGE: usize = 10;
const TIMEOUT_COVERAGE: Duration = Duration::from_secs(120);

const TOTAL_NUM_PEERS_STRESS: usize = 100;
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
    Noop,
}

/// Given a slice of handles assumed to be larger than 0,
/// chooses one
/// # Panics
/// panics if handles is of length 0
fn random_handle<S: Debug + Default + Send + Clone, T: NodeType>(
    handles: &[HandleWithState<S, T>],
    rng: &mut dyn rand::RngCore,
) -> HandleWithState<S, T> {
    handles.iter().choose(rng).unwrap().clone()
}

/// event handler for events from the swarm
/// - updates state based on events received
/// - replies to direct messages
#[instrument]
pub async fn counter_handle_network_event<T: NodeType>(
    event: NetworkEvent,
    handle: HandleWithState<CounterState, T>,
) -> Result<(), NetworkError> {
    use CounterMessage::*;
    use NetworkEvent::*;
    match event {
        IsBootstrapped | NetworkEvent::ConnectedPeersUpdate(..) => {}
        GossipMsg(m) | DirectResponse(m, _) => {
            if let Ok(msg) = bincode::deserialize::<CounterMessage>(&m) {
                match msg {
                    // direct message only
                    MyCounterIs(c) => {
                        *handle.state.lock().await = c;
                    }
                    // gossip message only
                    IncrementCounter { from, to, .. } => {
                        let mut state_lock = handle.state.lock().await;

                        if *state_lock == from {
                            *state_lock = to;
                        }

                        drop(state_lock);
                    }
                    // only as a response
                    AskForCounter | Noop => {}
                }
            } else {
                error!("FAILED TO DESERIALIZE MSG {:?}", m);
            }
        }
        DirectRequest(m, _, chan) => {
            if let Ok(msg) = bincode::deserialize::<CounterMessage>(&m) {
                match msg {
                    // direct message request
                    IncrementCounter { from, to, .. } => {
                        let mut state_lock = handle.state.lock().await;

                        if *state_lock == from {
                            *state_lock = to;
                        }

                        drop(state_lock);

                        handle.handle.direct_response(
                            chan,
                            &bincode::serialize(&CounterMessage::Noop).unwrap(),
                        )?;
                    }
                    // direct message response
                    AskForCounter => {
                        let response = MyCounterIs(*handle.state.lock().await);
                        handle
                            .handle
                            .direct_response(chan, &bincode::serialize(&response).unwrap())?;
                    }
                    MyCounterIs(_) => {
                        handle.handle.direct_response(
                            chan,
                            &bincode::serialize(&CounterMessage::Noop).unwrap(),
                        )?;
                    }
                    Noop => {
                        handle.handle.direct_response(
                            chan,
                            &bincode::serialize(&CounterMessage::Noop).unwrap(),
                        )?;
                    }
                }
            }
        }
    };
    Ok(())
}

/// `requester_handle` asks for `requestee_handle`'s state,
/// and then `requester_handle` updates its state to equal `requestee_handle`.
/// # Panics
/// on error
#[allow(clippy::similar_names)]
async fn run_request_response_increment<'a, T: NodeType>(
    requester_handle: HandleWithState<CounterState, T>,
    requestee_handle: HandleWithState<CounterState, T>,
    timeout: Duration,
) -> Result<(), TestError<CounterState>> {
    async move {
        let new_state = *requestee_handle.state.lock().await;
        let requestee_pid = requestee_handle.handle.peer_id();

        let state_ = Arc::clone(&requestee_handle.state);
        let stream = spawn(async move {
            let now = Instant::now();
            while Instant::now() - now < timeout {
                if *state_.lock().await == new_state {
                    return true;
                }
                sleep(Duration::from_millis(100)).await;
            }
            false
        });

        if !stream.await.unwrap() {
            error!("timed out waiting for {requestee_pid:?} to update state");
            std::process::exit(-1);
        }

        let state_ = Arc::clone(&requestee_handle.state);
        let stream = spawn(async move {
            let now = Instant::now();
            while Instant::now() - now < timeout {
                if *state_.lock().await == new_state {
                    return true;
                }
                sleep(Duration::from_millis(100)).await;
            }
            false
        });

        requester_handle
            .handle
            .direct_request(
                requestee_pid,
                &bincode::serialize(&CounterMessage::AskForCounter).unwrap(),
            )
            .map_err(|e| TestError::HandleError(format!("failed to send direct request: {e}")))?;

        if !stream.await.unwrap() {
            error!("timed out waiting for {requestee_pid:?} to update state");
            std::process::exit(-1);
        }

        let s1 = *requester_handle.state.lock().await;

        // sanity check
        if s1 == new_state {
            Ok(())
        } else {
            Err(TestError::InconsistentState {
                id: requester_handle.handle.id(),
                expected: new_state,
                actual: s1,
            })
        }
    }
    .await
}

/// broadcasts `msg` from a randomly chosen handle
/// then asserts that all nodes match `new_state`
async fn run_gossip_round<T: NodeType>(
    handles: &[HandleWithState<CounterState, T>],
    msg: CounterMessage,
    new_state: CounterState,
    timeout_duration: Duration,
) -> Result<(), TestError<CounterState>> {
    let mut rng = rand::thread_rng();
    let msg_handle = random_handle(handles, &mut rng);
    *msg_handle.state.lock().await = new_state;

    let mut join_set = JoinSet::new();

    let len = handles.len();
    for handle in handles {
        // already modified, so skip msg_handle
        if handle.handle.peer_id() != msg_handle.handle.peer_id() {
            let state_ = Arc::clone(&handle.state);
            let stream = spawn(async move {
                let now = Instant::now();
                while Instant::now() - now < timeout_duration {
                    if *state_.lock().await == new_state {
                        return true;
                    }
                    sleep(Duration::from_millis(100)).await;
                }
                false
            });

            join_set.spawn(stream);
        }
    }

    // make sure all are ready/listening
    for i in 0..len - 1 {
        match join_set.join_next().await.unwrap() {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => {
                error!("timed out waiting for handle {i:?} to subscribe to state events");
                std::process::exit(-1)
            }
            _ => {
                error!("error joining on stream");
                std::process::exit(-1)
            }
        }
    }

    msg_handle
        .handle
        .gossip("global".to_string(), &bincode::serialize(&msg).unwrap())
        .map_err(|e| TestError::HandleError(format!("failed to gossip: {e}")))?;

    for _ in 0..len - 1 {
        let _ = join_set.join_next().await.unwrap();
    }

    let mut failing = Vec::new();
    for handle in handles {
        let handle_state = *handle.state.lock().await;
        if handle_state != new_state {
            failing.push(handle.handle.id());
            println!("state: {handle_state:?}, expected: {new_state:?}");
        }
    }
    if !failing.is_empty() {
        let nodes = handles
            .iter()
            .cloned()
            .map(|h| h.handle)
            .collect::<Vec<_>>();
        print_connections(nodes.as_slice()).await;
        return Err(TestError::Timeout(failing, "gossiping".to_string()));
    }

    Ok(())
}

async fn run_intersperse_many_rounds<T: NodeType>(
    handles: Vec<HandleWithState<CounterState, T>>,
    timeout: Duration,
) {
    for i in 0..u32::try_from(NUM_ROUNDS).unwrap() {
        if i % 2 == 0 {
            run_request_response_increment_all(&handles, timeout).await;
        } else {
            run_gossip_rounds(&handles, 1, i, timeout).await;
        }
    }
    for h in handles {
        assert_eq!(*h.state.lock().await, u32::try_from(NUM_ROUNDS).unwrap());
    }
}

async fn run_dht_many_rounds<T: NodeType>(
    handles: Vec<HandleWithState<CounterState, T>>,
    timeout: Duration,
) {
    run_dht_rounds(&handles, timeout, 0, NUM_ROUNDS).await;
}

async fn run_dht_one_round<T: NodeType>(
    handles: Vec<HandleWithState<CounterState, T>>,
    timeout: Duration,
) {
    run_dht_rounds(&handles, timeout, 0, 1).await;
}

async fn run_request_response_many_rounds<T: NodeType>(
    handles: Vec<HandleWithState<CounterState, T>>,
    timeout: Duration,
) {
    for _i in 0..NUM_ROUNDS {
        run_request_response_increment_all(&handles, timeout).await;
    }
    for h in handles {
        assert_eq!(*h.state.lock().await, u32::try_from(NUM_ROUNDS).unwrap());
    }
}

/// runs one round of request response
/// # Panics
/// on error
async fn run_request_response_one_round<T: NodeType>(
    handles: Vec<HandleWithState<CounterState, T>>,
    timeout: Duration,
) {
    run_request_response_increment_all(&handles, timeout).await;
    for h in handles {
        assert_eq!(*h.state.lock().await, 1);
    }
}

/// runs multiple rounds of gossip
/// # Panics
/// on error
async fn run_gossip_many_rounds<T: NodeType>(
    handles: Vec<HandleWithState<CounterState, T>>,
    timeout: Duration,
) {
    run_gossip_rounds(&handles, NUM_ROUNDS, 0, timeout).await;
}

/// runs one round of gossip
/// # Panics
/// on error
async fn run_gossip_one_round<T: NodeType>(
    handles: Vec<HandleWithState<CounterState, T>>,
    timeout: Duration,
) {
    run_gossip_rounds(&handles, 1, 0, timeout).await;
}

/// runs many rounds of dht
/// # Panics
/// on error
async fn run_dht_rounds<T: NodeType>(
    handles: &[HandleWithState<CounterState, T>],
    timeout: Duration,
    _starting_val: usize,
    num_rounds: usize,
) {
    let mut rng = rand::thread_rng();
    for i in 0..num_rounds {
        debug!("begin round {}", i);
        let msg_handle = random_handle(handles, &mut rng);

        // Create a random keypair
        let mut rng = StdRng::from_entropy();
        let (public_key, private_key) =
            <T::SignatureKey as SignatureKey>::generated_from_seed_indexed(
                [1; 32],
                rng.gen::<u64>(),
            );

        // Create a random value to sign
        let value = (0..DHT_KV_PADDING)
            .map(|_| rng.gen::<u8>())
            .collect::<Vec<u8>>();

        // Create the record key
        let key = RecordKey::new(Namespace::Lookup, public_key.to_bytes().clone());

        // Sign the value
        let value = RecordValue::new_signed(&key, value, &private_key).expect("signing failed");

        // Put the key
        msg_handle
            .handle
            .put_record(key.clone(), value.clone())
            .await
            .unwrap();

        // get the key from the other nodes
        for handle in handles {
            let result: Result<Vec<u8>, NetworkError> =
                handle.handle.get_record_timeout(key.clone(), timeout).await;
            match result {
                Err(e) => {
                    error!("DHT error {e:?} during GET");
                    std::process::exit(-1);
                }
                Ok(v) => {
                    assert_eq!(v, value.value());
                }
            }
        }
    }
}

/// runs `num_rounds` of message broadcast, incrementing the state of all nodes each broadcast
async fn run_gossip_rounds<T: NodeType>(
    handles: &[HandleWithState<CounterState, T>],
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
#[allow(clippy::similar_names)]
async fn run_request_response_increment_all<T: NodeType>(
    handles: &[HandleWithState<CounterState, T>],
    timeout: Duration,
) {
    let mut rng = rand::thread_rng();
    let requestee_handle = random_handle(handles, &mut rng);
    *requestee_handle.state.lock().await += 1;
    info!("RR REQUESTEE IS {:?}", requestee_handle.handle.peer_id());
    let mut futs = Vec::new();
    for handle in handles {
        if handle
            .handle
            .lookup_pid(requestee_handle.handle.peer_id())
            .await
            .is_err()
        {
            error!("ERROR LOOKING UP REQUESTEE ADDRS");
        }
        // NOTE uncomment if debugging
        // let _ = h.print_routing_table().await;
        // skip `requestee_handle`
        if handle.handle.peer_id() != requestee_handle.handle.peer_id() {
            let requester_handle = handle.clone();
            futs.push(run_request_response_increment(
                requester_handle,
                requestee_handle.clone(),
                timeout,
            ));
        }
    }

    // NOTE this was originally join_all
    // but this is simpler.
    let results = Arc::new(RwLock::new(vec![]));

    let len = futs.len();

    for _ in 0..futs.len() {
        let fut = futs.pop().unwrap();
        let results = Arc::clone(&results);
        spawn(async move {
            let res = fut.await;
            results.write().await.push(res);
        });
    }
    loop {
        let l = results.read().await.iter().len();
        if l >= len {
            break;
        }
        info!("NUMBER OF RESULTS for increment all is: {}", l);
        sleep(Duration::from_secs(1)).await;
    }

    if results.read().await.iter().any(Result::is_err) {
        let nodes = handles
            .iter()
            .cloned()
            .map(|h| h.handle)
            .collect::<Vec<_>>();
        print_connections(nodes.as_slice()).await;
        let mut states = vec![];
        for handle in handles {
            states.push(*handle.state.lock().await);
        }
        error!("states: {states:?}");
        std::process::exit(-1);
    }
}

/// simple case of direct message

#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_coverage_request_response_one_round() {
    Box::pin(test_bed(
        run_request_response_one_round::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        TIMEOUT_COVERAGE,
    ))
    .await;
}

/// stress test of direct message

#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_coverage_request_response_many_rounds() {
    Box::pin(test_bed(
        run_request_response_many_rounds::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        TIMEOUT_COVERAGE,
    ))
    .await;
}

/// stress test of broadcast + direct message

#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_coverage_intersperse_many_rounds() {
    Box::pin(test_bed(
        run_intersperse_many_rounds::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        TIMEOUT_COVERAGE,
    ))
    .await;
}

/// stress teset that we can broadcast a message out and get counter increments

#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_coverage_gossip_many_rounds() {
    Box::pin(test_bed(
        run_gossip_many_rounds::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        TIMEOUT_COVERAGE,
    ))
    .await;
}

/// simple case of broadcast message

#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_coverage_gossip_one_round() {
    Box::pin(test_bed(
        run_gossip_one_round::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        TIMEOUT_COVERAGE,
    ))
    .await;
}

/// simple case of direct message

#[tokio::test(flavor = "multi_thread")]
#[instrument]
#[ignore]
async fn test_stress_request_response_one_round() {
    Box::pin(test_bed(
        run_request_response_one_round::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        TIMEOUT_STRESS,
    ))
    .await;
}

/// stress test of direct messsage

#[tokio::test(flavor = "multi_thread")]
#[instrument]
#[ignore]
async fn test_stress_request_response_many_rounds() {
    Box::pin(test_bed(
        run_request_response_many_rounds::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        TIMEOUT_STRESS,
    ))
    .await;
}

/// stress test of broadcast + direct message

#[tokio::test(flavor = "multi_thread")]
#[instrument]
#[ignore]
async fn test_stress_intersperse_many_rounds() {
    Box::pin(test_bed(
        run_intersperse_many_rounds::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        TIMEOUT_STRESS,
    ))
    .await;
}

/// stress teset that we can broadcast a message out and get counter increments

#[tokio::test(flavor = "multi_thread")]
#[instrument]
#[ignore]
async fn test_stress_gossip_many_rounds() {
    Box::pin(test_bed(
        run_gossip_many_rounds::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        TIMEOUT_STRESS,
    ))
    .await;
}

/// simple case of broadcast message

#[tokio::test(flavor = "multi_thread")]
#[instrument]
#[ignore]
async fn test_stress_gossip_one_round() {
    Box::pin(test_bed(
        run_gossip_one_round::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        TIMEOUT_STRESS,
    ))
    .await;
}

/// simple case of one dht publish event

#[tokio::test(flavor = "multi_thread")]
#[instrument]
#[ignore]
async fn test_stress_dht_one_round() {
    Box::pin(test_bed(
        run_dht_one_round::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        TIMEOUT_STRESS,
    ))
    .await;
}

/// many dht publishing events

#[tokio::test(flavor = "multi_thread")]
#[instrument]
#[ignore]
async fn test_stress_dht_many_rounds() {
    Box::pin(test_bed(
        run_dht_many_rounds::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_STRESS,
        TIMEOUT_STRESS,
    ))
    .await;
}

/// simple case of one dht publish event

#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_coverage_dht_one_round() {
    Box::pin(test_bed(
        run_dht_one_round::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        TIMEOUT_COVERAGE,
    ))
    .await;
}

/// many dht publishing events

#[tokio::test(flavor = "multi_thread")]
#[instrument]
async fn test_coverage_dht_many_rounds() {
    Box::pin(test_bed(
        run_dht_many_rounds::<TestTypes>,
        counter_handle_network_event,
        TOTAL_NUM_PEERS_COVERAGE,
        TIMEOUT_COVERAGE,
    ))
    .await;
}
