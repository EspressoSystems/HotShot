use crate::{FromBackground, Run};
use flume::Sender;
use futures::FutureExt;
use hotshot_types::traits::signature_key::{EncodedPublicKey, SignatureKey};
use std::collections::{BTreeMap, BTreeSet};
use tracing::debug;

pub struct Clients<K: SignatureKey>(Vec<BTreeMap<OrdKey<K>, Sender<FromBackground<K>>>>);

impl<K: SignatureKey + PartialEq> Clients<K> {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub async fn broadcast(&mut self, run: Run, msg: FromBackground<K>) {
        self.ensure_run_exists(run);
        let clients = &mut self.0[run.0];
        let futures = futures::future::join_all(clients.iter().map(|(id, sender)| {
            sender
                .send_async(msg.clone())
                .map(move |res| (id, res.is_ok()))
        }))
        .await;
        let keys_to_remove = futures
            .into_iter()
            .filter_map(|(id, is_ok)| if !is_ok { Some(id.clone()) } else { None })
            .collect();
        self.prune_nodes(run, keys_to_remove).await;
    }

    pub async fn broadcast_except_self(
        &mut self,
        run: Run,
        sender_key: K,
        message: FromBackground<K>,
    ) {
        self.ensure_run_exists(run);
        let clients = &mut self.0[run.0];
        let futures = futures::future::join_all(clients.iter().filter_map(|(id, sender)| {
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
        self.prune_nodes(run, keys_to_remove).await;
    }

    fn ensure_run_exists(&mut self, run: Run) {
        while self.0.len() <= run.0 {
            self.0.push(BTreeMap::new());
        }
    }

    pub async fn direct_message(&mut self, run: Run, receiver: K, msg: FromBackground<K>) {
        self.ensure_run_exists(run);
        let clients = &mut self.0[run.0];
        let receiver = OrdKey::from(receiver);
        if let Some(sender) = clients.get_mut(&receiver) {
            if sender.send_async(msg).await.is_err() {
                let mut tree = BTreeSet::new();
                tree.insert(receiver);
                self.prune_nodes(run, tree).await;
            }
        }
    }

    async fn prune_nodes(&mut self, run: Run, mut clients_with_error: BTreeSet<OrdKey<K>>) {
        self.ensure_run_exists(run);
        // While notifying the clients of other clients disconnecting, those clients can be disconnected too
        // we solve this by looping over this until we've removed all failing nodes and have successfully notified everyone else.
        let clients = &mut self.0[run.0];
        while !clients_with_error.is_empty() {
            let clients_to_remove = std::mem::take(&mut clients_with_error);
            for client in &clients_to_remove {
                debug!("Background task could not deliver message to client thread {:?}, removing them", client.pubkey);
                clients.remove(client);
            }
            let mut futures = Vec::with_capacity(clients.len() * clients_to_remove.len());
            for client in clients_to_remove {
                let message = FromBackground::node_disconnected(client.key);
                futures.extend(clients.iter().map(|(id, sender)| {
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

    pub fn len(&self, run: Run) -> usize {
        self.0.get(run.0).map(|r| r.len()).unwrap_or(0)
    }

    pub fn insert(&mut self, run: Run, key: K, sender: Sender<FromBackground<K>>) {
        self.ensure_run_exists(run);
        self.0[run.0].insert(key.into(), sender);
    }

    pub fn remove(&mut self, run: Run, key: K) {
        self.ensure_run_exists(run);
        let key = OrdKey::from(key);
        self.0[run.0].remove(&key);
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
