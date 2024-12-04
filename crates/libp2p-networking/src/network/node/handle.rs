// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{collections::HashSet, fmt::Debug, marker::PhantomData, time::Duration};

use anyhow::Context;
use hotshot_types::traits::{
    network::NetworkError, node_implementation::NodeType, signature_key::SignatureKey,
};
use libp2p::request_response::ResponseChannel;
use libp2p_identity::PeerId;
use tokio::{
    sync::mpsc::{Receiver, UnboundedReceiver, UnboundedSender},
    time::{sleep, timeout},
};
use tracing::{info, instrument};

use crate::network::{
    behaviours::dht::record::{Namespace, RecordKey, RecordValue},
    ClientRequest, NetworkEvent, NetworkNode,
};

use super::config::Libp2pConfig;

/// A handle containing:
/// - A reference to the state
/// - Controls for the swarm
#[derive(Debug, Clone)]
pub struct NetworkNodeHandle<S: SignatureKey> {
    /// For sending requests to the network behaviour
    request_sender: UnboundedSender<ClientRequest>,

    /// Phantom data
    pd: PhantomData<S>,
}

/// internal network node receiver
#[derive(Debug)]
pub struct NetworkNodeReceiver {
    /// The receiver for requests from the application
    request_receiver: UnboundedReceiver<NetworkEvent>,

    /// The kill switch for the receiver
    recv_kill: Option<Receiver<()>>,
}

impl NetworkNodeReceiver {
    /// recv a network event
    /// # Errors
    /// Errors if the receiver channel is closed
    pub async fn recv(&mut self) -> Result<NetworkEvent, NetworkError> {
        self.request_receiver
            .recv()
            .await
            .ok_or(NetworkError::ChannelReceiveError(
                "Receiver channel closed".to_string(),
            ))
    }
    /// Add a kill switch to the receiver
    pub fn set_kill_switch(&mut self, kill_switch: Receiver<()>) {
        self.recv_kill = Some(kill_switch);
    }

    /// Take the kill switch to allow killing the receiver task
    pub fn take_kill_switch(&mut self) -> Option<Receiver<()>> {
        self.recv_kill.take()
    }
}

/// Spawn a network node task task and return the handle and the receiver for it
/// # Errors
/// Errors if spawning the task fails
pub async fn spawn_network_node<T: NodeType>(
    config: Libp2pConfig<T>,
) -> anyhow::Result<(NetworkNodeReceiver, NetworkNodeHandle<T::SignatureKey>)> {
    // Create the network node (what we send requests to and from in the application)
    let mut network = NetworkNode::new(&config)
        .await
        .with_context(|| "failed to create network node")?;

    // Bind the swarm to the given address
    network
        .bind_to(&config.bind_address)
        .await
        .with_context(|| format!("failed to bind to {:?}", config.bind_address))?;

    // Spawn the listeners and get the request sender and receiver
    let (request_sender, request_receiver) = network
        .spawn_listeners()
        .with_context(|| "failed to spawn listeners")?;

    // Create the receiver
    let receiver = NetworkNodeReceiver {
        request_receiver,
        recv_kill: None,
    };

    // Create the handle (what the application uses to interact with the network)
    let handle = NetworkNodeHandle {
        request_sender,
        pd: PhantomData,
    };

    Ok((receiver, handle))
}

impl<S: SignatureKey + 'static> NetworkNodeHandle<S> {
    /// Cleanly shuts down a swarm node
    /// This is done by sending a message to
    /// the swarm itself to spin down
    #[instrument]
    pub async fn shutdown(&self) -> Result<(), NetworkError> {
        self.send_request(ClientRequest::Shutdown)?;
        Ok(())
    }
    /// Notify the network to begin the bootstrap process
    /// # Errors
    /// If unable to send via `send_network`. This should only happen
    /// if the network is shut down.
    pub fn begin_bootstrap(&self) -> Result<(), NetworkError> {
        let req = ClientRequest::BeginBootstrap;
        self.send_request(req)
    }

    /// Print out the routing table used by kademlia
    /// NOTE: only for debugging purposes currently
    /// # Errors
    /// if the client has stopped listening for a response
    pub async fn print_routing_table(&self) -> Result<(), NetworkError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::GetRoutingTable(s);
        self.send_request(req)?;
        r.await
            .map_err(|e| NetworkError::ChannelReceiveError(e.to_string()))
    }
    /// Wait until at least `num_peers` have connected
    ///
    /// # Errors
    /// If the channel closes before the result can be sent back
    pub async fn wait_to_connect(&self, num_required_peers: usize) -> Result<(), NetworkError> {
        // Wait for the required number of peers to connect
        loop {
            // Get the number of currently connected peers
            let num_connected = self.num_connected().await?;
            if num_connected >= num_required_peers {
                break;
            }

            // Log the number of connected peers
            info!(
                "Libp2p connected to {}/{} peers",
                num_connected, num_required_peers
            );

            // Sleep for a second before checking again
            sleep(Duration::from_secs(1)).await;
        }

        Ok(())
    }

    /// Look up a peer's addresses in kademlia
    /// NOTE: this should always be called before any `request_response` is initiated
    /// # Errors
    /// if the client has stopped listening for a response
    pub async fn lookup_pid(&self, peer_id: PeerId) -> Result<(), NetworkError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::LookupPeer(peer_id, s);
        self.send_request(req)?;
        r.await
            .map_err(|err| NetworkError::ChannelReceiveError(err.to_string()))
    }

    /// Looks up a node's `PeerId` by its staking key. Is authenticated through
    /// `get_record` assuming each record should be signed.
    ///
    /// # Errors
    /// If the DHT lookup fails
    pub async fn lookup_node(
        &self,
        key: &[u8],
        dht_timeout: Duration,
    ) -> Result<PeerId, NetworkError> {
        // Create the record key
        let key = RecordKey::new(Namespace::Lookup, key.to_vec());

        // Get the record from the DHT
        let pid = self.get_record_timeout(key, dht_timeout).await?;

        PeerId::from_bytes(&pid).map_err(|err| NetworkError::FailedToDeserialize(err.to_string()))
    }

    /// Insert a record into the kademlia DHT
    /// # Errors
    /// - Will return [`NetworkError::FailedToSerialize`] when unable to serialize the key or value
    pub async fn put_record(
        &self,
        key: RecordKey,
        value: RecordValue<S>,
    ) -> Result<(), NetworkError> {
        // Serialize the key
        let key = key.to_bytes();

        // Serialize the record
        let value = bincode::serialize(&value)
            .map_err(|e| NetworkError::FailedToSerialize(e.to_string()))?;

        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::PutDHT {
            key: key.clone(),
            value,
            notify: s,
        };

        self.send_request(req)?;

        r.await.map_err(|_| NetworkError::RequestCancelled)
    }

    /// Receive a record from the kademlia DHT if it exists.
    /// Must be replicated on at least 2 nodes
    /// # Errors
    /// - Will return [`NetworkError::FailedToSerialize`] when unable to serialize the key
    /// - Will return [`NetworkError::FailedToDeserialize`] when unable to deserialize the returned value
    pub async fn get_record(
        &self,
        key: RecordKey,
        retry_count: u8,
    ) -> Result<Vec<u8>, NetworkError> {
        // Serialize the key
        let serialized_key = key.to_bytes();

        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::GetDHT {
            key: serialized_key.clone(),
            notify: s,
            retry_count,
        };
        self.send_request(req)?;

        // Map the error
        let result = r.await.map_err(|_| NetworkError::RequestCancelled)?;

        // Deserialize the record's value
        let record: RecordValue<S> = bincode::deserialize(&result)
            .map_err(|e| NetworkError::FailedToDeserialize(e.to_string()))?;

        Ok(record.value().to_vec())
    }

    /// Get a record from the kademlia DHT with a timeout
    /// # Errors
    /// - Will return [`NetworkError::Timeout`] when times out
    /// - Will return [`NetworkError::FailedToSerialize`] when unable to serialize the key or value
    /// - Will return [`NetworkError::ChannelSendError`] when underlying `NetworkNode` has been killed
    pub async fn get_record_timeout(
        &self,
        key: RecordKey,
        timeout_duration: Duration,
    ) -> Result<Vec<u8>, NetworkError> {
        timeout(timeout_duration, self.get_record(key, 3))
            .await
            .map_err(|err| NetworkError::Timeout(err.to_string()))?
    }

    /// Insert a record into the kademlia DHT with a timeout
    /// # Errors
    /// - Will return [`NetworkError::Timeout`] when times out
    /// - Will return [`NetworkError::FailedToSerialize`] when unable to serialize the key or value
    /// - Will return [`NetworkError::ChannelSendError`] when underlying `NetworkNode` has been killed
    pub async fn put_record_timeout(
        &self,
        key: RecordKey,
        value: RecordValue<S>,
        timeout_duration: Duration,
    ) -> Result<(), NetworkError> {
        timeout(timeout_duration, self.put_record(key, value))
            .await
            .map_err(|err| NetworkError::Timeout(err.to_string()))?
    }

    /// Subscribe to a topic
    /// # Errors
    /// - Will return [`NetworkError::ChannelSendError`] when underlying `NetworkNode` has been killed
    pub async fn subscribe(&self, topic: String) -> Result<(), NetworkError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::Subscribe(topic, Some(s));
        self.send_request(req)?;
        r.await
            .map_err(|err| NetworkError::ChannelReceiveError(err.to_string()))
    }

    /// Unsubscribe from a topic
    /// # Errors
    /// - Will return [`NetworkError::ChannelSendError`] when underlying `NetworkNode` has been killed
    pub async fn unsubscribe(&self, topic: String) -> Result<(), NetworkError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::Unsubscribe(topic, Some(s));
        self.send_request(req)?;
        r.await
            .map_err(|err| NetworkError::ChannelReceiveError(err.to_string()))
    }

    /// Ignore `peers` when pruning
    /// e.g. maintain their connection
    /// # Errors
    /// - Will return [`NetworkError::ChannelSendError`] when underlying `NetworkNode` has been killed
    pub fn ignore_peers(&self, peers: Vec<PeerId>) -> Result<(), NetworkError> {
        let req = ClientRequest::IgnorePeers(peers);
        self.send_request(req)
    }

    /// Make a direct request to `peer_id` containing `msg`
    /// # Errors
    /// - Will return [`NetworkError::ChannelSendError`] when underlying `NetworkNode` has been killed
    /// - Will return [`NetworkError::FailedToSerialize`] when unable to serialize `msg`
    pub fn direct_request(&self, pid: PeerId, msg: &[u8]) -> Result<(), NetworkError> {
        self.direct_request_no_serialize(pid, msg.to_vec())
    }

    /// Make a direct request to `peer_id` containing `msg` without serializing
    /// # Errors
    /// - Will return [`NetworkError::ChannelSendError`] when underlying `NetworkNode` has been killed
    /// - Will return [`NetworkError::FailedToSerialize`] when unable to serialize `msg`
    pub fn direct_request_no_serialize(
        &self,
        pid: PeerId,
        contents: Vec<u8>,
    ) -> Result<(), NetworkError> {
        let req = ClientRequest::DirectRequest {
            pid,
            contents,
            retry_count: 1,
        };
        self.send_request(req)
    }

    /// Reply with `msg` to a request over `chan`
    /// # Errors
    /// - Will return [`NetworkError::ChannelSendError`] when underlying `NetworkNode` has been killed
    /// - Will return [`NetworkError::FailedToSerialize`] when unable to serialize `msg`
    pub fn direct_response(
        &self,
        chan: ResponseChannel<Vec<u8>>,
        msg: &[u8],
    ) -> Result<(), NetworkError> {
        let req = ClientRequest::DirectResponse(chan, msg.to_vec());
        self.send_request(req)
    }

    /// Forcefully disconnect from a peer
    /// # Errors
    /// If the channel is closed somehow
    /// Shouldnt' happen.
    /// # Panics
    /// If channel errors out
    /// shouldn't happen.
    pub fn prune_peer(&self, pid: PeerId) -> Result<(), NetworkError> {
        let req = ClientRequest::Prune(pid);
        self.send_request(req)
    }

    /// Gossip a message to peers
    /// # Errors
    /// - Will return [`NetworkError::ChannelSendError`] when underlying `NetworkNode` has been killed
    /// - Will return [`NetworkError::FailedToSerialize`] when unable to serialize `msg`
    pub fn gossip(&self, topic: String, msg: &[u8]) -> Result<(), NetworkError> {
        self.gossip_no_serialize(topic, msg.to_vec())
    }

    /// Gossip a message to peers without serializing
    /// # Errors
    /// - Will return [`NetworkError::ChannelSendError`] when underlying `NetworkNode` has been killed
    /// - Will return [`NetworkError::FailedToSerialize`] when unable to serialize `msg`
    pub fn gossip_no_serialize(&self, topic: String, msg: Vec<u8>) -> Result<(), NetworkError> {
        let req = ClientRequest::GossipMsg(topic, msg);
        self.send_request(req)
    }

    /// Send a client request to the network
    ///
    /// # Errors
    /// - Will return [`NetworkError::ChannelSendError`] when underlying `NetworkNode` has been killed
    fn send_request(&self, req: ClientRequest) -> Result<(), NetworkError> {
        self.request_sender
            .send(req)
            .map_err(|err| NetworkError::ChannelSendError(err.to_string()))
    }

    /// Returns number of peers this node is connected to
    /// # Errors
    /// If the channel is closed somehow
    /// Shouldnt' happen.
    /// # Panics
    /// If channel errors out
    /// shouldn't happen.
    pub async fn num_connected(&self) -> Result<usize, NetworkError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::GetConnectedPeerNum(s);
        self.send_request(req)?;
        Ok(r.await.unwrap())
    }

    /// return hashset of PIDs this node is connected to
    /// # Errors
    /// If the channel is closed somehow
    /// Shouldnt' happen.
    /// # Panics
    /// If channel errors out
    /// shouldn't happen.
    pub async fn connected_pids(&self) -> Result<HashSet<PeerId>, NetworkError> {
        let (s, r) = futures::channel::oneshot::channel();
        let req = ClientRequest::GetConnectedPeers(s);
        self.send_request(req)?;
        Ok(r.await.unwrap())
    }
}
