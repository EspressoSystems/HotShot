use futures_lite::{future, FutureExt};
use futures_locks::RwLock;
use serde::{de::DeserializeOwned, Serialize};
use snafu::{ensure, OptionExt};

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::networking::{ListenerSend, NetworkingImplementation, NoSuchNode};
use crate::PubKey;

#[derive(Clone)]
pub struct MemoryNetwork<T: Clone> {
    broadcast: Arc<RwLock<HashMap<PubKey, VecDeque<T>>>>,
    direct: Arc<RwLock<HashMap<PubKey, VecDeque<T>>>>,
    nodes: Arc<parking_lot::RwLock<HashSet<PubKey>>>,
    node_id: Option<PubKey>,
}

impl<T: Clone> MemoryNetwork<T> {
    pub fn new(node_id: PubKey) -> Self {
        let x = Self::new_listener();
        x.add_node(node_id)
    }

    pub fn new_listener() -> Self {
        Self {
            broadcast: Arc::new(RwLock::new(HashMap::new())),
            direct: Arc::new(RwLock::new(HashMap::new())),
            node_id: None,
            nodes: Arc::new(parking_lot::RwLock::new(HashSet::new())),
        }
    }

    pub fn add_node(&self, node_id: PubKey) -> Self {
        let mut x = self.clone();
        x.node_id = Some(node_id.clone());
        {
            let nodes: &mut HashSet<PubKey> = &mut x.nodes.write();
            nodes.insert(node_id);
        }
        x
    }

    pub fn add_listener(&self) -> Self {
        let mut x = self.clone();
        x.node_id = None;
        x
    }
}

impl<T: Clone + Serialize + DeserializeOwned + Send + 'static> NetworkingImplementation<T>
    for MemoryNetwork<T>
{
    fn broadcast_message(&self, message: T) -> future::Boxed<Result<(), super::NetworkError>> {
        let broadcast = self.broadcast.clone();
        let nodes: Vec<PubKey> = self.nodes.read().iter().cloned().collect();
        let is_node = self.node_id.is_some();
        async move {
            ensure!(is_node, ListenerSend);
            let b: &mut HashMap<PubKey, VecDeque<T>> = &mut *broadcast.write().await;
            for node in nodes {
                // Make sure there is a bucket
                if !b.contains_key(&node) {
                    b.insert(node.clone(), VecDeque::new());
                }
                // This unwrap is safe since we ensured the bucket exists
                b.get_mut(&node).unwrap().push_back(message.clone());
            }
            Ok(())
        }
        .boxed()
    }

    fn message_node(
        &self,
        message: T,
        recipient: PubKey,
    ) -> future::Boxed<Result<(), super::NetworkError>> {
        let direct = self.direct.clone();
        let node_exists = self.nodes.read().contains(&recipient);
        let is_node = self.node_id.is_some();
        async move {
            ensure!(is_node, ListenerSend);
            ensure!(node_exists, NoSuchNode);
            let d: &mut HashMap<PubKey, VecDeque<T>> = &mut *direct.write().await;
            // make sure there is a bucket
            if !d.contains_key(&recipient) {
                d.insert(recipient.clone(), VecDeque::new());
            }
            // This unwrap is safe since we made sure the bucket exists
            d.get_mut(&recipient).unwrap().push_back(message);

            Ok(())
        }
        .boxed()
    }

    fn broadcast_queue(&self) -> future::Boxed<Result<Vec<T>, super::NetworkError>> {
        let broadcast = self.broadcast.clone();
        let node_id = self.node_id.clone();
        async move {
            let node_id = node_id.context(ListenerSend)?;
            let b: &mut HashMap<PubKey, VecDeque<T>> = &mut *broadcast.write().await;
            if let Some(bucket) = b.get_mut(&node_id) {
                let vec = bucket.drain(..).collect();
                Ok(vec)
            } else {
                // No data here, just return an empty vec
                Ok(Vec::new())
            }
        }
        .boxed()
    }

    fn next_broadcast(&self) -> future::Boxed<Result<Option<T>, super::NetworkError>> {
        let broadcast = self.broadcast.clone();
        let node_id = self.node_id.clone();
        async move {
            let node_id = node_id.context(ListenerSend)?;
            let b: &mut HashMap<PubKey, VecDeque<T>> = &mut *broadcast.write().await;
            if let Some(bucket) = b.get_mut(&node_id) {
                let it = bucket.pop_front();
                Ok(it)
            } else {
                // No data here, just return an empty vec
                Ok(None)
            }
        }
        .boxed()
    }

    fn direct_queue(&self) -> future::Boxed<Result<Vec<T>, super::NetworkError>> {
        let direct = self.direct.clone();
        let node_id = self.node_id.clone();
        async move {
            let node_id = node_id.context(ListenerSend)?;
            let d: &mut HashMap<PubKey, VecDeque<T>> = &mut *direct.write().await;
            if let Some(bucket) = d.get_mut(&node_id) {
                let vec = bucket.drain(..).collect();
                Ok(vec)
            } else {
                // No data here, just return an empty vec
                Ok(Vec::new())
            }
        }
        .boxed()
    }

    fn next_direct(&self) -> future::Boxed<Result<Option<T>, super::NetworkError>> {
        let direct = self.direct.clone();
        let node_id = self.node_id.clone();
        async move {
            let node_id = node_id.context(ListenerSend)?;
            let d: &mut HashMap<PubKey, VecDeque<T>> = &mut *direct.write().await;
            if let Some(bucket) = d.get_mut(&node_id) {
                let it = bucket.pop_front();
                Ok(it)
            } else {
                // No data here, just return an empty vec
                Ok(None)
            }
        }
        .boxed()
    }

    fn known_nodes(&self) -> future::Boxed<Vec<PubKey>> {
        let nodes = self.nodes.read().iter().cloned().collect();
        async move { nodes }.boxed()
    }

    fn obj_clone(&self) -> Box<dyn NetworkingImplementation<T> + 'static> {
        Box::new(self.clone())
    }
}
