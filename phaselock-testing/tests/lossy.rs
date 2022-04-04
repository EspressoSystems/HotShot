#![cfg(feature = "demo")]
#![allow(clippy::type_complexity)]
mod common;
use common::*;
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use tracing::{debug, error, info, instrument, trace, warn};

use rand::distributions::{Bernoulli, Distribution, Uniform};
use rand::seq::IteratorRandom;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Sub;
use std::time::Duration;

use phaselock::{
    demos::dentry::*,
    tc,
    traits::{
        implementations::{MasterMap, MemoryNetwork, MemoryStorage, Stateless},
        NetworkReliability,
    },
    types::{Event, EventType, Message, PhaseLockHandle},
    PhaseLock, PhaseLockConfig, PubKey, H_256,
};

#[allow(clippy::upper_case_acronyms)]
type NODE = DEntryNode<MemoryNetwork<Message<DEntryBlock, Transaction, State, H_256>>>;

/// A synchronous network. Packets may be delayed, but are guaranteed
/// to arrive within `timeout` ns
#[derive(Clone, Copy, Debug)]
pub struct SynchronousNetwork {
    /// max delay of packet before arrival
    timeout_ms: u64,
    /// lowest value in milliseconds that a packet may be delayed
    delay_low_ms: u64,
}

impl NetworkReliability for SynchronousNetwork {
    /// never drop a packet
    fn sample_keep(&self) -> bool {
        true
            asdfakdf
    }
    fn sample_delay(&self) -> Duration {
        Duration::from_millis(
            Uniform::new_inclusive(self.delay_low_ms, self.timeout_ms)
                .sample(&mut rand::thread_rng()),
        )
    }
}

/// An asynchronous network. Packets may be dropped entirely
/// or delayed for arbitrarily long periods
/// probability that packet is kept = `keep_numerator` / `keep_denominator`
/// packet delay is obtained by sampling from a uniform distribution
/// between `delay_low_ms` and `delay_high_ms`, inclusive
#[derive(Debug, Clone, Copy)]
pub struct AsynchronousNetwork {
    /// numerator for probability of keeping packets
    keep_numerator: u32,
    /// denominator for probability of keeping packets
    keep_denominator: u32,
    /// lowest value in milliseconds that a packet may be delayed
    delay_low_ms: u64,
    /// highest value in milliseconds that a packet may be delayed
    delay_high_ms: u64,
}

impl NetworkReliability for AsynchronousNetwork {
    fn sample_keep(&self) -> bool {
        Bernoulli::from_ratio(self.keep_numerator, self.keep_denominator)
            .unwrap()
            .sample(&mut rand::thread_rng())
    }
    fn sample_delay(&self) -> Duration {
        Duration::from_millis(
            Uniform::new_inclusive(self.delay_low_ms, self.delay_high_ms)
                .sample(&mut rand::thread_rng()),
        )
    }
}

/// An partially synchronous network. Behaves asynchronously
/// until some arbitrary time bound, GST,
/// then synchronously after GST
#[derive(Debug, Clone, Copy)]
pub struct PartiallySynchronousNetwork {
    /// asynchronous portion of network
    asynchronous: AsynchronousNetwork,
    /// synchronous portion of network
    synchronous: SynchronousNetwork,
    /// time when GST occurs
    gst: std::time::Duration,
    /// when the network was started
    start: std::time::Instant,
}

impl NetworkReliability for PartiallySynchronousNetwork {
    /// never drop a packet
    fn sample_keep(&self) -> bool {
        true
    }
    fn sample_delay(&self) -> Duration {
        // act asyncronous before gst
        if self.start.elapsed() < self.gst {
            if self.asynchronous.sample_keep() {
                self.asynchronous.sample_delay()
            } else {
                // assume packet was "dropped" and will arrive after gst
                self.synchronous.sample_delay() + self.gst
            }
        } else {
            // act syncronous after gst
            self.synchronous.sample_delay()
        }
    }
}

#[allow(clippy::derivable_impls)]
impl Default for SynchronousNetwork {
    // disable all chance of failure
    fn default() -> Self {
        SynchronousNetwork {
            delay_low_ms: 0,
            timeout_ms: 0,
        }
    }
}

impl Default for AsynchronousNetwork {
    // disable all chance of failure
    fn default() -> Self {
        AsynchronousNetwork {
            keep_numerator: 1,
            keep_denominator: 1,
            delay_low_ms: 0,
            delay_high_ms: 0,
        }
    }
}

impl Default for PartiallySynchronousNetwork {
    fn default() -> Self {
        PartiallySynchronousNetwork {
            synchronous: SynchronousNetwork::default(),
            asynchronous: AsynchronousNetwork::default(),
            gst: std::time::Duration::new(0, 0),
            start: std::time::Instant::now(),
        }
    }
}

impl SynchronousNetwork {
    /// create new `SynchronousNetwork`
    pub fn new(timeout: u64, delay_low_ms: u64) -> Self {
        SynchronousNetwork {
            timeout_ms: timeout,
            delay_low_ms,
        }
    }
}

impl AsynchronousNetwork {
    /// create new `AsynchronousNetwork`
    pub fn new(
        keep_numerator: u32,
        keep_denominator: u32,
        delay_low_ms: u64,
        delay_high_ms: u64,
    ) -> Self {
        AsynchronousNetwork {
            keep_numerator,
            keep_denominator,
            delay_low_ms,
            delay_high_ms,
        }
    }
}

impl PartiallySynchronousNetwork {
    /// create new `PartiallySynchronousNetwork`
    pub fn new(
        asynchronous: AsynchronousNetwork,
        synchronous: SynchronousNetwork,
        gst: std::time::Duration,
    ) -> Self {
        PartiallySynchronousNetwork {
            asynchronous,
            synchronous,
            gst,
            start: std::time::Instant::now(),
        }
    }
}

/// This runs multiple rounds rounds of consensus
/// with all networks treated with `network_reliability`.
/// ensuring 'num_txns` randomly generated transactions
/// become committed between 3 * `num_byzantine` + 1 nodes.
/// safety check: at the end of each view, any nodes
///   that commit should commit the same final block and state
/// terminiation check: once all nodes have completed view i,
///   at least one node must have each of the generated `important_txns`
///   in storage, and all nodes must have produced a Decide event
///   in a more recent view than all of the `important_txns`
#[instrument]
async fn lossy_network(
    network_reliability: impl NetworkReliability + 'static,
    num_txns: usize,
    num_byzantine: usize,
) {
    setup_logging();

    // Calculate the threshold
    let threshold = 2 * num_byzantine + 1;
    let num_nodes = 3 * num_byzantine + 1;
    info!(?num_nodes, ?threshold);
    // Generate the private key set
    // Generated using xoshiro for reproduceability
    let mut rng = Xoshiro256StarStar::seed_from_u64(0);
    let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut rng);
    // Generate the networking backends
    let master = MasterMap::<Message<DEntryBlock, Transaction, State, H_256>>::new();
    let mut networkings: Vec<(
        MemoryNetwork<Message<DEntryBlock, Transaction, State, H_256>>,
        PubKey,
    )> = Vec::new();
    for node_id in 0..num_nodes {
        let pub_key = PubKey::from_secret_key_set_escape_hatch(&sks, node_id.try_into().unwrap());
        let mn = MemoryNetwork::new(pub_key.clone(), master.clone(), Some(network_reliability));
        networkings.push((mn, pub_key));
    }
    info!("Created networking");
    // Create the phaselocks
    let known_nodes: Vec<PubKey> = networkings.iter().map(|(_, x)| x.clone()).collect();
    let config = PhaseLockConfig {
        total_nodes: num_nodes as u32,
        threshold: threshold as u32,
        max_transactions: 100,
        known_nodes,
        next_view_timeout: 100,
        timeout_ratio: (11, 10),
        round_start_delay: 1,
        start_delay: 1,
    };
    debug!(?config);
    let gensis = DEntryBlock::default();
    let accounts = (b'a'..=b'g')
        .map(|c| c as char) // Convert all to chars
        .filter(|c| c.is_alphabetic()) // Filter only alphabetic chars
        .map(|c| char::to_string(&c))
        .collect::<Vec<String>>();
    let state = State {
        balances: accounts
            .clone()
            .iter()
            .map(|name| (name.clone(), 500_000_000))
            .collect::<BTreeMap<Account, Balance>>(),
        nonces: BTreeSet::default(),
    };
    let mut phaselocks: Vec<PhaseLockHandle<NODE, H_256>> = Vec::new();
    for node_id in 0..num_nodes {
        let h = PhaseLock::init(
            gensis.clone(),
            sks.public_keys(),
            sks.secret_key_share(node_id),
            node_id.try_into().unwrap(),
            config.clone(),
            state.clone(),
            networkings[node_id as usize].0.clone(),
            MemoryStorage::default(),
            Stateless::default(),
        )
        .await
        .expect("Could not init phaselock");
        phaselocks.push(h);
    }

    let important_txns: Vec<_> =
        random_transactions(&mut rand::thread_rng(), num_txns, accounts.clone())
            .into_iter()
            .collect();

    info!("PhaseLocks prepared, running prebaked transactions");
    let mut round = 1;
    let mut txns = important_txns.clone();
    let mut mrc = vec![0; num_nodes];
    let mut txns_to_view = HashMap::new();
    while !check_if_finished(&important_txns, &txns_to_view, &mrc) {
        let txn = match txns.pop() {
            Some(t) => t,
            None => {
                // accounts nonempty, so unwrap is fine
                random_transactions(&mut rand::thread_rng(), 1, accounts.clone())
                    .into_iter()
                    .next()
                    .unwrap()
            }
        };

        phaselocks
            .iter()
            .choose(&mut rand::thread_rng())
            .unwrap()
            .submit_transaction(txn.clone())
            .await
            .expect("Failed to submit transaction");

        // add second random transaction to prevent round from spinning
        // this txn is not tracked and is only used for liveness
        phaselocks
            .iter()
            .choose(&mut rand::thread_rng())
            .unwrap()
            // phaselocks nonempty, so unwrap is fine
            .submit_transaction(
                random_transactions(&mut rand::thread_rng(), 1, accounts.clone())
                    .into_iter()
                    .next()
                    .unwrap(),
            )
            .await
            .expect("Failed to submit transaction");

        for (_i, phaselock) in phaselocks.iter().enumerate() {
            phaselock.run_one_round().await;
        }
        debug!("Waiting for consensus to occur");
        let mut num_failed = 0;
        let mut states = HashMap::<usize, Vec<State>>::new();
        let mut blocks = HashMap::<usize, Vec<DEntryBlock>>::new();
        let mut successful_nodes = HashSet::new();
        for (node_id, phaselock) in phaselocks.iter_mut().enumerate() {
            debug!(?node_id, "Waiting on node to emit decision");
            let mut event: Event<DEntryBlock, State> = phaselock
                .next_event()
                .await
                .expect("PhaseLock unexpectedly closed");
            // Actually wait for decision
            while !matches!(event.event, EventType::Decide { .. } if event.view_number == round ) {
                info!("lossy: next event for node id {:?}", node_id);
                // timeout -> exist
                if let EventType::ViewTimeout { view_number } = event.event {
                    if view_number < round {
                        continue;
                    }

                    warn!(?event, "\nRound timed out for replica {:?}\n", node_id,);
                    num_failed += 1;
                    break;
                }
                // error -> continue
                else if matches!(event.event, EventType::Error { .. }) {
                    warn!(?event, "\nRound encountered error {:?}\n", event.event);
                    num_failed += 1;
                    break;
                }
                // decide from prior view shouldn't be possible
                else if let EventType::Decide { .. } = event.event {
                    error!(
                        "decision event from previous round {:?} encountered in round {:?}",
                        event.view_number, round
                    );
                }

                trace!(?node_id, ?event);
                event = phaselock
                    .next_event()
                    .await
                    .expect("PhaseLock unexpectedly closed");
            }
            if let EventType::Decide { block, state } = event.event {
                warn!("\nRound finished for replica {:?}\n", node_id);
                debug!(?node_id, "Node reached decision");
                // commit for round
                successful_nodes.insert(node_id);
                states.insert(node_id, (*state).clone());
                blocks.insert(node_id, (*block).clone());
                mrc[node_id] = round as usize;
            } else {
                error!(
                    "round failed for replica {} with error {:?}",
                    node_id, event,
                );
            }
        }
        error!(
            "Round finished. {:?} failures occurred on nodes: {:?}",
            num_failed,
            (0..num_nodes)
                .into_iter()
                .collect::<HashSet<_>>()
                .sub(&successful_nodes)
        );
        // update when/if transaction was committed
        if contains_txn(&txn, &blocks) {
            // transaction was committed by at least one replice
            txns_to_view.insert(txn, round as usize);
        } else {
            // transaction failed. Resubmit, and try again
            txns.push(txn);
        }
        check_safety(&successful_nodes, &states, &blocks).await;

        // Finally, increment the round counter
        round += 1;
    }
}

/// checks safety requirement. In particular, checks that the most recent
/// block and state for a view match across all nodes that committed
/// `nodes_to_check`: set of node ids that committed this view
/// `node_states`: map node_id -> commited Vec<State> for this view
/// `node_blocks`: map node_id -> commited Vec<DEntryBlock> for this view
/// # Panics
/// Panics if node has no state or blocks included
pub async fn check_safety(
    nodes_to_check: &HashSet<usize>,
    node_states: &HashMap<usize, Vec<State>>,
    node_blocks: &HashMap<usize, Vec<DEntryBlock>>,
) {
    if nodes_to_check.len() <= 1 {
        return;
    }

    let first_node_idx = nodes_to_check.iter().next().unwrap();

    let first_blocks = node_blocks[first_node_idx].clone();
    let first_states = node_states[first_node_idx].clone();

    for &i in nodes_to_check {
        let i_blocks = node_blocks[&i].clone();
        let i_states = node_states[&i].clone();
        // first block/state most recent
        if first_blocks.get(0) != i_blocks.get(0) || first_states.get(0) != i_states.get(0) {
            error!(
                ?first_blocks,
                ?i_blocks,
                ?first_states,
                ?i_states,
                ?first_node_idx,
                ?i,
                "SAFETY ERROR: most recent block or state does not match"
            );
            panic!("safety check failed");
        }
    }
}

/// checks that `txn` is committed in at least one node within the map
/// `blocks` (from `node_id` -> blocks) generated during a view
pub fn contains_txn(txn: &Transaction, blocks: &HashMap<usize, Vec<DEntryBlock>>) -> bool {
    for (_k, node_blocks) in blocks.iter() {
        for node_block in node_blocks {
            if node_block.transactions.contains(txn) {
                return true;
            }
        }
    }
    false
}

/// `txn_to_view` is a map from Transaction to the view it was committed
/// `mrc` is a map from node id to the most recent view that committed
/// this function checks the termination condition that all txns
/// have been committed by all nodes
pub fn check_if_finished(
    txns: &[Transaction],
    txn_to_view: &HashMap<Transaction, usize>,
    mrc: &[usize],
) -> bool {
    let mut most_recent_view = 0;
    // check that all txns have been committed
    // find the txn with the highest view
    for txn in txns {
        match txn_to_view.get(txn) {
            None => return false,
            Some(view) => most_recent_view = most_recent_view.max(*view),
        }
    }

    // check that all nodes have committed a view more recent than any of the submitted txns last committed view
    mrc.iter().all(|&view| view > most_recent_view)
}

#[async_std::test]
#[instrument]
async fn test_no_loss_network() {
    // tests base level of working synchronous network
    lossy_network(SynchronousNetwork::default(), 1, 10).await
}

#[async_std::test]
#[instrument]
async fn test_synchronous_network() {
    // tests network with forced packet delay
    lossy_network(SynchronousNetwork::new(10, 5), 2, 4).await
}

#[async_std::test]
#[instrument]
async fn test_asynchronous_network() {
    // tests network with small packet delay and dropped packets
    lossy_network(AsynchronousNetwork::new(97, 100, 0, 5), 2, 4).await
}

/// tests network with asynchronous patch that eventually becomes synchronous
#[async_std::test]
#[instrument]
async fn test_partially_synchronous_network() {
    let asn = AsynchronousNetwork::new(90, 100, 0, 0);
    let sn = SynchronousNetwork::new(10, 0);
    let gst = std::time::Duration::new(10, 0);
    lossy_network(PartiallySynchronousNetwork::new(asn, sn, gst), 2, 4).await
}
