use crate::FromServer;
use flume::Sender;
use futures::FutureExt;
use hotshot_types::traits::signature_key::{EncodedPublicKey, SignatureKey};
use std::collections::{BTreeMap, BTreeSet};
use tracing::debug;

pub struct Clients<K: SignatureKey>(BTreeMap<OrdKey<K>, Sender<FromServer<K>>>);

impl<K: SignatureKey + PartialEq> Clients<K> {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub async fn broadcast(&mut self, msg: FromServer<K>) {
        let futures = futures::future::join_all(self.0.iter().map(|(id, sender)| {
            sender
                .send_async(msg.clone())
                .map(move |res| (id, res.is_ok()))
        }))
        .await;
        let keys_to_remove = futures
            .into_iter()
            .filter_map(|(id, is_ok)| if !is_ok { Some(id.clone()) } else { None })
            .collect();
        self.prune_nodes(keys_to_remove).await;
    }

    pub async fn broadcast_except_self(&mut self, sender_key: K, message: FromServer<K>) {
        let futures = futures::future::join_all(self.0.iter().filter_map(|(id, sender)| {
            if id.key != sender_key {
                Some(
                    sender
                        .send_async(message.clone())
                        .map(move |res| (id, res.is_ok())),
                )
            } else {
                None
            }
        }))
        .await;
        let keys_to_remove = futures
            .into_iter()
            .filter_map(|(id, is_ok)| if !is_ok { Some(id.clone()) } else { None })
            .collect();
        self.prune_nodes(keys_to_remove).await;
    }

    pub async fn direct_message(&mut self, receiver: K, msg: FromServer<K>) {
        let receiver = OrdKey::from(receiver);
        if let Some(sender) = self.0.get_mut(&receiver) {
            if sender.send_async(msg).await.is_err() {
                let mut tree = BTreeSet::new();
                tree.insert(receiver);
                self.prune_nodes(tree).await;
            }
        }
    }

    async fn prune_nodes(&mut self, mut clients_with_error: BTreeSet<OrdKey<K>>) {
        // While notifying the clients of other clients disconnecting, those clients can be disconnected too
        // we solve this by looping over this until we've removed all failing nodes and have successfully notified everyone else.
        while !clients_with_error.is_empty() {
            let clients_to_remove = std::mem::take(&mut clients_with_error);
            for client in &clients_to_remove {
                debug!("Background task could not deliver message to client thread {:?}, removing them", client.pubkey);
                self.0.remove(client);
            }
            let mut futures = Vec::with_capacity(self.0.len() * clients_to_remove.len());
            for client in clients_to_remove {
                let message = FromServer::NodeDisconnected { key: client.key };
                futures.extend(self.0.iter().map(|(id, sender)| {
                    sender
                        .send_async(message.clone())
                        .map(move |result| (id, result.is_ok()))
                }));
            }
            let results = futures::future::join_all(futures).await;
            clients_with_error = results
                .into_iter()
                .filter_map(|(id, is_ok)| if !is_ok { Some(id.clone()) } else { None })
                .collect();
        }
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn insert(&mut self, key: K, sender: Sender<FromServer<K>>) {
        self.0.insert(key.into(), sender);
    }

    pub fn remove(&mut self, key: K) {
        let key = OrdKey::from(key);
        self.0.remove(&key);
    }
}

#[derive(PartialEq, Eq, Clone)]
struct OrdKey<K: SignatureKey> {
    key: K,
    pubkey: EncodedPublicKey,
}

impl<K: SignatureKey> From<K> for OrdKey<K> {
    fn from(key: K) -> Self {
        let pubkey = key.to_bytes();
        Self { key, pubkey }
    }
}

impl<K: SignatureKey> PartialOrd for OrdKey<K> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.pubkey.partial_cmp(&other.pubkey)
    }
}
impl<K: SignatureKey> Ord for OrdKey<K> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.pubkey.cmp(&other.pubkey)
    }
}
