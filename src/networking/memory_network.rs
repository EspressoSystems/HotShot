use dashmap::{DashMap, DashSet};
use futures::FutureExt;
use serde::{de::DeserializeOwned, Serialize};
use snafu::{ensure, OptionExt};

use std::collections::VecDeque;
use std::sync::Arc;

use super::BoxedFuture;
use crate::networking::{ListenerSend, NetworkingImplementation, NoSuchNode};
use crate::PubKey;

#[derive(Clone)]
/// In memory network emulator
pub struct MemoryNetwork<T: Clone> {
    /// Broadcast queue for each node
    broadcast: Arc<DashMap<PubKey, VecDeque<T>>>,
    /// Direct queue for each nodes
    direct: Arc<DashMap<PubKey, VecDeque<T>>>,
    /// The list of nodes
    nodes: Arc<DashSet<PubKey>>,
    /// The ID for this node
    node_id: Option<PubKey>,
}

impl<T: Clone> MemoryNetwork<T> {
    /// Creates a new `MemoryNetwork` and loads this node into it
    pub fn new(node_id: PubKey) -> Self {
        let x = Self::new_listener();
        x.add_node(node_id)
    }

    /// Creates a new `MemoryNetwork` that can only receive broadcasts
    pub fn new_listener() -> Self {
        Self {
            broadcast: Arc::new(DashMap::new()),
            direct: Arc::new(DashMap::new()),
            node_id: None,
            nodes: Arc::new(DashSet::new()),
        }
    }

    /// Creates a clone of the existing `MemoryNetwork`, but with a new node associated
    pub fn add_node(&self, node_id: PubKey) -> Self {
        let mut x = self.clone();
        x.node_id = Some(node_id.clone());
        x.nodes.insert(node_id);
        x
    }

    /// Creates a clone of the existing `MemoryNetwork` that can only receive broadcasts
    pub fn add_listener(&self) -> Self {
        let mut x = self.clone();
        x.node_id = None;
        x
    }
}

impl<T: Clone + Serialize + DeserializeOwned + Send + Sync + 'static> NetworkingImplementation<T>
    for MemoryNetwork<T>
{
    fn broadcast_message(&self, message: T) -> BoxedFuture<Result<(), super::NetworkError>> {
        let broadcast = self.broadcast.clone();
        let nodes: Vec<PubKey> = self.nodes.iter().map(|x| x.clone()).collect();
        let is_node = self.node_id.is_some();
        async move {
            ensure!(is_node, ListenerSend);
            for node in nodes {
                // Make sure there is a bucket
                if !broadcast.contains_key(&node) {
                    broadcast.insert(node.clone(), VecDeque::new());
                }
                // This unwrap is safe since we ensured the bucket exists
                broadcast.get_mut(&node).unwrap().push_back(message.clone());
            }
            Ok(())
        }
        .boxed()
    }

    fn message_node(
        &self,
        message: T,
        recipient: PubKey,
    ) -> BoxedFuture<Result<(), super::NetworkError>> {
        let direct = self.direct.clone();
        let node_exists = self.nodes.contains(&recipient);
        let is_node = self.node_id.is_some();
        async move {
            ensure!(is_node, ListenerSend);
            ensure!(node_exists, NoSuchNode);
            // make sure there is a bucket
            if !direct.contains_key(&recipient) {
                direct.insert(recipient.clone(), VecDeque::new());
            }
            // This unwrap is safe since we made sure the bucket exists
            direct.get_mut(&recipient).unwrap().push_back(message);

            Ok(())
        }
        .boxed()
    }

    fn broadcast_queue(&self) -> BoxedFuture<Result<Vec<T>, super::NetworkError>> {
        let broadcast = self.broadcast.clone();
        let node_id = self.node_id.clone();
        async move {
            let node_id = node_id.context(ListenerSend)?;
            if let Some(mut bucket) = broadcast.get_mut(&node_id) {
                let vec = bucket.drain(..).collect();
                Ok(vec)
            } else {
                // No data here, just return an empty vec
                Ok(Vec::new())
            }
        }
        .boxed()
    }

    fn next_broadcast(&self) -> BoxedFuture<Result<Option<T>, super::NetworkError>> {
        let broadcast = self.broadcast.clone();
        let node_id = self.node_id.clone();
        async move {
            let node_id = node_id.context(ListenerSend)?;
            if let Some(mut bucket) = broadcast.get_mut(&node_id) {
                let it = bucket.pop_front();
                Ok(it)
            } else {
                // No data here, just return an empty vec
                Ok(None)
            }
        }
        .boxed()
    }

    fn direct_queue(&self) -> BoxedFuture<Result<Vec<T>, super::NetworkError>> {
        let direct = self.direct.clone();
        let node_id = self.node_id.clone();
        async move {
            let node_id = node_id.context(ListenerSend)?;
            if let Some(mut bucket) = direct.get_mut(&node_id) {
                let vec = bucket.drain(..).collect();
                Ok(vec)
            } else {
                // No data here, just return an empty vec
                Ok(Vec::new())
            }
        }
        .boxed()
    }

    fn next_direct(&self) -> BoxedFuture<Result<Option<T>, super::NetworkError>> {
        let direct: Arc<DashMap<PubKey, VecDeque<T>>> = self.direct.clone();
        let node_id = self.node_id.clone();
        async move {
            let node_id = node_id.context(ListenerSend)?;
            if let Some(mut bucket) = direct.get_mut(&node_id) {
                let it = bucket.pop_front();
                Ok(it)
            } else {
                // No data here, just return an empty vec
                Ok(None)
            }
        }
        .boxed()
    }

    fn known_nodes(&self) -> BoxedFuture<Vec<PubKey>> {
        let nodes = self.nodes.iter().map(|x| x.clone()).collect();
        async move { nodes }.boxed()
    }

    fn obj_clone(&self) -> Box<dyn NetworkingImplementation<T> + 'static> {
        Box::new(self.clone())
    }
}
