/// `BlockContents` implementation for the double entry bookkeeping demo
pub mod block;

use crate::message::Message;
use crate::networking::w_network::WNetwork;
use crate::{HotStuff, HotStuffConfig, PubKey};
use blake3::Hasher;
use block::{Addition, DEntryBlock, State, Subtraction, Transaction};
use rand::Rng;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::BTreeMap;
use threshold_crypto as tc;

/// Attempts to create a network connection with a random port
pub async fn try_network<
    T: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static,
>(
    key: PubKey,
) -> (WNetwork<T>, u16) {
    // TODO: Actually attempt to open the port and find a new one if it doens't work
    let port = rand::thread_rng().gen_range(2000, 5000);
    (
        WNetwork::new_from_strings(key, vec![], port, None)
            .await
            .expect("Failed to create network"),
        port,
    )
}

/// Generates the `SecretKeySet` for this BFT instance
pub fn gen_keys(threshold: usize) -> tc::SecretKeySet {
    tc::SecretKeySet::random(threshold, &mut rand::thread_rng())
}

/// Turns a `PublicKeySet` into a set of `HotStuff` `PubKey`s
pub fn set_to_keys(total: usize, set: &tc::PublicKeySet) -> Vec<PubKey> {
    (0..total)
        .map(|x| PubKey {
            set: set.clone(),
            node: set.public_key_share(x),
            nonce: x as u64,
        })
        .collect()
}

/// Attempts to create a hotstuff instance
pub async fn try_hotstuff(
    keys: &tc::SecretKeySet,
    total: usize,
    threshold: usize,
    node_number: usize,
) -> (
    HotStuff<DEntryBlock>,
    PubKey,
    u16,
    WNetwork<Message<DEntryBlock, Transaction>>,
) {
    // TODO !corbett how do all the genesis blocks get initialized?
    assert_eq!(
        *Hasher::new().finalize().as_bytes(),
        *Hasher::new().finalize().as_bytes()
    );
    let genesis = DEntryBlock {
        previous_block: *Hasher::new().finalize().as_bytes(),
        transactions: vec![],
    };
    let pub_key_set = keys.public_keys();
    let tc_pub_key = pub_key_set.public_key_share(node_number);
    let pub_key = PubKey {
        set: pub_key_set.clone(),
        node: tc_pub_key,
        nonce: node_number as u64,
    };
    let config = HotStuffConfig {
        total_nodes: total as u32,
        thershold: threshold as u32,
        max_transactions: 100,
        known_nodes: set_to_keys(total, &pub_key_set),
    };
    let (networking, port) = try_network(pub_key.clone()).await;
    let hotstuff = HotStuff::new(
        genesis,
        &keys,
        node_number as u64,
        config,
        State {
            balances: BTreeMap::new(),
        },
        networking.clone(),
    );
    (hotstuff, pub_key, port, networking)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::utility::test_util::setup_logging;
    use async_std::task::spawn;
    use futures::channel::oneshot;
    use futures::future::join_all;

    #[async_std::test]
    async fn hotstuff_dentry_demo() {
        setup_logging();
        let keys = gen_keys(3);
        // Create the hotstuffs and spawn their tasks
        let hotstuffs: Vec<(HotStuff<DEntryBlock>, PubKey, u16, WNetwork<_>)> =
            join_all((0..5).map(|x| try_hotstuff(&keys, 5, 4, x))).await;
        // Boot up all the low level networking implementations
        for (_, _, _, network) in &hotstuffs {
            let (x, sync) = oneshot::channel();
            match network.generate_task(x) {
                Some(task) => {
                    spawn(task);
                    sync.await.expect("sync.await failed");
                }
                None => {
                    println!("generate_task(x) returned None");
                    panic!();
                }
            }
        }
        // Connect the hotstuffs
        for (i, (_, key, port, _)) in hotstuffs.iter().enumerate() {
            let socket = format!("localhost:{}", port);
            // Loop through all the other hotstuffs and connect it to this one
            for (_, key_2, port_2, network_2) in &hotstuffs[i..] {
                println!("Connecting {} to {}", port_2, port);
                if key != key_2 {
                    network_2
                        .connect_to(key.clone(), &socket)
                        .await
                        .expect("Unable to connect to node");
                }
            }
        }
        // Boot up all the high level implementations
        for (hotstuff, _, _, _) in &hotstuffs {
            hotstuff.spawn_networking_tasks().await;
        }
        // Wait for all nodes to connect to each other
        println!("Waiting for nodes to fully connect");
        for (_, _, _, w) in &hotstuffs {
            while w.connection_table_size().await < 4 {
                async_std::task::sleep(std::time::Duration::from_millis(10)).await;
            }
            while w.nodes_table_size().await < 4 {
                async_std::task::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
        println!("Nodes should be connected");
        println!(
            "Current states: {:?}",
            join_all(hotstuffs.iter().map(|(h, _, _, _)| h.get_state())).await
        );
        // Propose a new transaction
        println!("Proposing first transaction");
        hotstuffs[0]
            .0
            .publish_transaction_async(Transaction {
                add: Addition {
                    account: "Alice".to_string(),
                    amount: 0,
                },
                sub: Subtraction {
                    account: "Bob".to_string(),
                    amount: 0,
                },
            })
            .await
            .unwrap();
        // issuing new views
        println!("Issuing new view messages");
        join_all(hotstuffs.iter().map(|(h, _, _, _)| h.next_view(0))).await;
        // Running a round of consensus
        println!("Running round 1");
        join_all(hotstuffs.iter().map(|(h, _, _, _)| h.run_round(1))).await;
        println!(
            "Current states: {:?}",
            join_all(hotstuffs.iter().map(|(h, _, _, _)| h.get_state())).await
        );
        // Propose a new transaction
        println!("Proposing second transaction");
        hotstuffs[1]
            .0
            .publish_transaction_async(Transaction {
                add: Addition {
                    account: "Alice".to_string(),
                    amount: 0,
                },
                sub: Subtraction {
                    account: "Bob".to_string(),
                    amount: 0,
                },
            })
            .await
            .unwrap();
        // issuing new views
        println!("Issuing new view messages");
        join_all(hotstuffs.iter().map(|(h, _, _, _)| h.next_view(1))).await;
        // Running a round of consensus
        println!("Running round 2");
        join_all(hotstuffs.iter().map(|(h, _, _, _)| h.run_round(2))).await;
        println!(
            "Current states: {:?}",
            join_all(hotstuffs.iter().map(|(h, _, _, _)| h.get_state())).await
        );
        // Propose a new transaction
        println!("Proposing third transaction");
        hotstuffs[0]
            .0
            .publish_transaction_async(Transaction {
                add: Addition {
                    account: "Alice".to_string(),
                    amount: 0,
                },
                sub: Subtraction {
                    account: "Bob".to_string(),
                    amount: 0,
                },
            })
            .await
            .unwrap();
        // issuing new views
        println!("Issuing new view messages");
        join_all(hotstuffs.iter().map(|(h, _, _, _)| h.next_view(2))).await;
        // Running a round of consensus
        println!("Running round 3");
        join_all(hotstuffs.iter().map(|(h, _, _, _)| h.run_round(3))).await;
        println!(
            "Current states: {:?}",
            join_all(hotstuffs.iter().map(|(h, _, _, _)| h.get_state())).await
        );
    }
}
