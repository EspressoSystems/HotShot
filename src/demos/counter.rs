mod block;

use block::*;

use rand::Rng;
use serde::{de::DeserializeOwned, Serialize};
use threshold_crypto as tc;

use crate::message::Message;
use crate::networking::w_network::WNetwork;
use crate::{HotStuff, HotStuffConfig, PubKey};

fn gen_keys(threshold: usize) -> tc::SecretKeySet {
    tc::SecretKeySet::random(threshold, &mut rand::thread_rng())
}

async fn try_network<
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

fn set_to_keys(total: usize, set: &tc::PublicKeySet) -> Vec<PubKey> {
    (0..total)
        .map(|x| PubKey {
            set: set.clone(),
            node: set.public_key_share(x),
            nonce: x as u64,
        })
        .collect()
}

async fn make_counter_validator(
    keys: &tc::SecretKeySet,
    total: usize,
    threshold: usize,
    node_number: usize,
) -> (
    HotStuff<CounterBlock>,
    PubKey,
    u16,
    WNetwork<Message<CounterBlock, CounterTransaction>>,
) {
    let genesis = CounterBlock {
        tx: [CounterTransaction::Genesis { state: 0 }].to_vec(),
    };
    let pub_key_set = keys.public_keys();
    let tc_pub_key = pub_key_set.public_key_share(node_number);
    let pub_key = PubKey {
        set: pub_key_set.clone(),
        node: tc_pub_key.clone(),
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
        0,
        networking.clone(),
    );
    (hotstuff, pub_key, port, networking)
}

#[cfg(test)]
mod test {
    use super::*;
    use async_std::task::spawn;
    use futures::channel::oneshot;
    use futures::future::join_all;

    /// A low validator count for testing
    const VALIDATOR_COUNT: usize = 5;

    /// Calculates the number of signatures required to meet the
    /// threshold for threshold cryptography.
    ///
    /// Note, the threshold_crypto crate internally adds one to this
    /// value. It takes one more signature than the threshold to
    /// generate a threshold signature.
    fn calc_signature_threshold(validator_count: usize) -> usize {
        (2 * validator_count) / 3 + 1
    }

    #[async_std::test]
    async fn spawn_one_hotstuff() {
        let threshold = calc_signature_threshold(1);
        // Nathan M, this calls gen_keys(0), but before it called
        // gen_keys(1). Test still passes.
        let keys = gen_keys(threshold - 1);
        let (_hotstuff, _pub_key, _port, _networking) =
            make_counter_validator(&keys, VALIDATOR_COUNT, threshold, 0).await;
    }

    #[async_std::test]
    async fn make_counter_validator_demo() {
        let threshold = calc_signature_threshold(VALIDATOR_COUNT);
        let keys = gen_keys(threshold - 1);
        // Create the hotstuffs and spawn their tasks
        let hotstuffs: Vec<(HotStuff<CounterBlock>, PubKey, u16, WNetwork<_>)> = join_all(
            (0..VALIDATOR_COUNT)
                .map(|ix| make_counter_validator(&keys, VALIDATOR_COUNT, threshold, ix)),
        )
        .await;
        // Boot up all the low level networking implementations
        for (_, _, _, network) in &hotstuffs {
            let (ix, sync) = oneshot::channel();
            match network.generate_task(ix) {
                Some(task) => {
                    spawn(task);
                    sync.await.expect("sync.await failed");
                }
                None => {
                    println!("generate_task(ix) returned None");
                    panic!();
                }
            }
        }
        // Connect the hotstuffs
        for (ix, (_, key, port, _)) in hotstuffs.iter().enumerate() {
            let socket = format!("localhost:{}", port);
            // Loop through all the other hotstuffs and connect it to this one
            for (_, key_2, port_2, network_2) in &hotstuffs[ix..] {
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
            while w.connection_table_size().await < VALIDATOR_COUNT - 1 {
                async_std::task::sleep(std::time::Duration::from_millis(10)).await;
            }
            while w.nodes_table_size().await < VALIDATOR_COUNT - 1 {
                async_std::task::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
        println!("Nodes should be connected");
        println!(
            "Current states: {:?}",
            join_all(hotstuffs.iter().map(|(h, _, _, _)| h.get_state())).await
        );
        // Propose a new transaction
        println!("Proposing to increment from 0 -> 1");
        hotstuffs[0]
            .0
            .publish_transaction_async(CounterTransaction::Inc { previous: 0 })
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
        println!("Proposing to increment from 1 -> 2");
        hotstuffs[1]
            .0
            .publish_transaction_async(CounterTransaction::Inc { previous: 1 })
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
        println!("Proposing to increment from 2 -> 3");
        hotstuffs[0]
            .0
            .publish_transaction_async(CounterTransaction::Inc { previous: 2 })
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

    #[async_std::test]
    async fn transaction_mock() {
        assert!(true);
    }
}
