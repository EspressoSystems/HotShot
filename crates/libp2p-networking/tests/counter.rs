// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

#![allow(clippy::panic)]

mod common;
use std::{fmt::Debug, sync::Arc, time::Duration};

use common::{test_bed, HandleWithState, TestError};
use hotshot_example_types::node_types::TestTypes;
use hotshot_types::traits::{network::NetworkError, node_implementation::NodeType};
use libp2p_networking::network::NetworkEvent;
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};
use tokio::{
    spawn,
    task::JoinSet,
    time::{sleep, Instant},
};
use tracing::{error, info, instrument, warn};

use crate::common::print_connections;

pub type CounterState = u32;

const NUM_ROUNDS: usize = 100;

const TOTAL_NUM_PEERS_COVERAGE: usize = 10;
const TIMEOUT_COVERAGE: Duration = Duration::from_secs(120);

const TOTAL_NUM_PEERS_STRESS: usize = 100;
const TIMEOUT_STRESS: Duration = Duration::from_secs(60);

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

/// broadcasts `msg` from a randomly chosen handle
/// then asserts that all nodes match `new_state`
async fn run_gossip_round<T: NodeType>(
    handles: &[HandleWithState<CounterState, T>],
    msg: CounterMessage,
    new_state: CounterState,
    timeout_duration: Duration,
) -> Result<(), TestError> {
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
