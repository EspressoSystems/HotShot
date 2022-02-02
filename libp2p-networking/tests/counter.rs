use std::{sync::Arc, time::Duration};

use async_std::task::sleep;
use bincode::Options;

use libp2p::gossipsub::Topic;
use networking_demo::{
    network_node::{ClientRequest, NetworkEvent},
    network_node_handle::{
        get_random_handle, test_bed, HandlerError, NetworkNodeHandle, SendSnafu, SerializationSnafu,
    },
};
use rand::{seq::IteratorRandom, thread_rng};
use std::fmt::Debug;

use serde::{Deserialize, Serialize};

use snafu::ResultExt;

use tracing::{error, instrument, warn};

pub type Counter = u8;

const TOTAL_NUM_PEERS: usize = 20;

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

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Default)]
pub struct CounterState(Counter);

/// event handler for events from the swarm
/// - updates state based on events received
/// - replies to direct messages
#[instrument]
pub async fn counter_handle_network_event(
    event: NetworkEvent,
    handle: Arc<NetworkNodeHandle<CounterState>>,
) -> Result<(), HandlerError> {
    use CounterMessage::*;
    #[allow(clippy::enum_glob_use)]
    use NetworkEvent::*;
    let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
    match event {
        GossipMsg(m) | DirectResponse(m) => {
            if let Ok(msg) = bincode_options.deserialize::<CounterMessage>(&m) {
                match msg {
                    MyCounterIs(c) | CounterMessage::IncrementCounter { to: c, .. } => {
                        *handle.state.lock().await = c;
                    }
                    AskForCounter => {}
                }
            }
        },
        DirectRequest(m, chan) => {
            if let Ok(msg) = bincode_options.deserialize::<CounterMessage>(&m) {
                match msg {
                    IncrementCounter { to, .. } => {
                        *handle.state.lock().await = to;
                    }
                    AskForCounter => {
                        let response = MyCounterIs(handle.state.lock().await.clone());
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
        },
        UpdateConnectedPeers(p) => {
            handle.connection_state.lock().await.connected_peers = p;
        },
        UpdateKnownPeers(p) => {
            handle.connection_state.lock().await.known_peers = p;
        },
        SuccessfulBootstrap(_) => {}
    };
    Ok(())
}

/// check that we can direct message to increment counter
#[async_std::test]
#[instrument]
async fn test_request_response() {
    async fn run_request_response(handles: Vec<Arc<NetworkNodeHandle<CounterState>>>) {
        let send_handle = get_random_handle(handles.as_slice());
        let recv_handle = get_random_handle(handles.as_slice());

        *send_handle.state.lock().await = CounterState(5);

        let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
        let msg_inner = bincode_options
            .serialize(&CounterMessage::IncrementCounter {
                from: CounterState(0),
                to: CounterState(5),
            })
            .unwrap();
        let msg = ClientRequest::DirectRequest(recv_handle.peer_id, msg_inner);
        send_handle.send_network.send_async(msg).await.unwrap();

        // block to let the direction message
        // TODO make this event driven
        // e.g. everyone receives the gossipmsg event
        // or timeout
        sleep(Duration::from_secs(1)).await;

        for handle in handles.iter() {
            let expected_state =
                if handle.peer_id == send_handle.peer_id || handle.peer_id == recv_handle.peer_id {
                    CounterState(5)
                } else {
                    CounterState::default()
                };
            assert_eq!(*handle.state.lock().await, expected_state);
        }
    }

    test_bed(
        run_request_response,
        counter_handle_network_event,
        TOTAL_NUM_PEERS,
    )
    .await
}

/// check that we can broadcast a message out and get counter increments
#[async_std::test]
#[instrument]
async fn test_gossip() {
    async fn run_gossip(handles: Vec<Arc<NetworkNodeHandle<CounterState>>>) {
        let msg_handle = handles.iter().choose(&mut thread_rng()).unwrap();
        *msg_handle.state.lock().await = CounterState(5);
        let bincode_options = bincode::DefaultOptions::new().with_limit(16_384);
        let msg_inner = bincode_options
            .serialize(&CounterMessage::IncrementCounter {
                from: CounterState(0),
                to: CounterState(5),
            })
            .unwrap();
        let (send, recv) = flume::bounded(1);
        let msg = ClientRequest::GossipMsg(Topic::new("global"), msg_inner, send);
        msg_handle.send_network.send_async(msg).await.unwrap();
        recv.recv_async().await.unwrap().unwrap();

        // block to let the gossipping happen
        // TODO make this event driven
        // e.g. everyone receives the gossipmsg event
        // or timeout
        sleep(Duration::from_secs(10)).await;

        let mut failing_idxs = Vec::new();
        for (i, handle) in handles.iter().enumerate() {
            if *handle.state.lock().await != CounterState(5) {
                failing_idxs.push(i);
            }
        }
        if !failing_idxs.is_empty() {
            error!(?failing_idxs, "failing idxs!!");
            panic!("some nodes did not receive the message {:?}", failing_idxs);
        }
    }

    test_bed(run_gossip, counter_handle_network_event, TOTAL_NUM_PEERS).await;
}
