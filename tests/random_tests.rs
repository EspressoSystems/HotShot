#![allow(clippy::type_complexity)]

mod common;

use phaselock::{
    demos::dentry::*,
    tc,
    traits::implementations::{
        DummyReliability, MasterMap, MemoryNetwork, MemoryStorage, Stateless,
    },
    types::{Event, EventType, Message, PhaseLockHandle},
    PhaseLock, PhaseLockConfig, PhaseLockError, PubKey, H_256,
};
use proptest::prelude::*;
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::iter::FromIterator;
use tracing::{debug, error, instrument, warn};

const NEXT_VIEW_TIMEOUT: u64 = 100;
const DEFAULT_TIMEOUT_RATIO: (u64, u64) = (15, 10);
const SEED: u64 = 1234;

#[allow(clippy::upper_case_acronyms)]
type NODE = DEntryNode<MemoryNetwork<Message<DEntryBlock, Transaction, State, H_256>>>;

/// Errors when trying to reach consensus.
#[derive(Debug)]
pub enum ConsensusError {
    /// View times out with any node as the leader.
    TimedOutWithAnyLeader,

    FailedToProposeTxn,

    PhaselockClosed(PhaseLockError),

    /// States after a round of consensus is inconsistent.
    InconsistentAfterTxn,
}

fn get_threshold(num_nodes: u64) -> u64 {
    ((num_nodes * 2) / 3) + 1
}

fn get_tolerance(num_nodes: u64) -> u64 {
    num_nodes - get_threshold(num_nodes)
}

/// Gets networking backends of all nodes.
async fn get_networkings<
    T: Clone + Serialize + DeserializeOwned + Send + Sync + std::fmt::Debug + 'static,
>(
    num_nodes: u64,
    sks: &tc::SecretKeySet,
) -> Vec<(MemoryNetwork<T>, PubKey)> {
    let master = MasterMap::<T>::new();
    let mut networkings: Vec<(MemoryNetwork<T>, PubKey)> = Vec::new();
    for node_id in 0..num_nodes {
        let pub_key = PubKey::from_secret_key_set_escape_hatch(sks, node_id);
        let network = MemoryNetwork::new(
            pub_key.clone(),
            master.clone(),
            Option::<DummyReliability>::None,
        );
        networkings.push((network, pub_key));
    }
    networkings
}

fn init_state() -> State {
    // Create the initial state
    let balances: BTreeMap<Account, Balance> = vec![
        ("Joe", 1_000_000),
        ("Nathan M", 500_000),
        ("John", 400_000),
        ("Nathan Y", 600_000),
        ("Ian", 100),
    ]
    .into_iter()
    .map(|(x, y)| (x.to_string(), y))
    .collect();
    State {
        balances,
        nonces: BTreeSet::default(),
    }
}

/// Creates the initial state and phaselocks.
///
/// Returns the initial state and the phaselocks of unfailing nodes.
///
/// # Arguments
///
/// * `nodes_to_fail` - a set of nodes to be failed, i.e., nodes whose
/// phaselocks will never get unpaused, and a boolean indicating whether
/// to fail the first or last `num_failed_nodes` nodes.
#[instrument(skip(sks, networkings))]
async fn init_state_and_phaselocks(
    sks: &tc::SecretKeySet,
    num_nodes: u64,
    nodes_to_fail: HashSet<u64>,
    threshold: u64,
    networkings: Vec<(
        MemoryNetwork<Message<DEntryBlock, Transaction, State, H_256>>,
        PubKey,
    )>,
    updated_timeout_ratio: Option<(u64, u64)>,
) -> (State, Vec<PhaseLockHandle<NODE, H_256>>) {
    let state = init_state();

    // Create the initial phaselocks
    let known_nodes: Vec<_> = (0..num_nodes)
        .map(|x| PubKey::from_secret_key_set_escape_hatch(sks, x))
        .collect();
    let timeout_ratio = match updated_timeout_ratio {
        Some(time) => time,
        None => DEFAULT_TIMEOUT_RATIO,
    };
    let config = PhaseLockConfig {
        total_nodes: num_nodes as u32,
        threshold: threshold as u32,
        max_transactions: 100,
        known_nodes,
        next_view_timeout: NEXT_VIEW_TIMEOUT,
        timeout_ratio,
        round_start_delay: 1,
        start_delay: 1,
    };
    debug!(?config);
    let genesis = DEntryBlock::default();
    let mut phaselocks = Vec::new();
    for node_id in 0..num_nodes {
        let (_, phaselock) = PhaseLock::init(
            genesis.clone(),
            sks.public_keys(),
            sks.secret_key_share(node_id),
            node_id,
            config.clone(),
            state.clone(),
            networkings[node_id as usize].0.clone(),
            MemoryStorage::default(),
            Stateless::default(),
        )
        .await
        .expect("Could not init phaselock");
        if !nodes_to_fail.contains(&node_id) {
            phaselocks.push(phaselock);
        }
        debug!("phaselock launched");
    }

    (state, phaselocks)
}

/// Provides a random valid transaction from the current state.
fn random_transaction<R: rand::Rng>(state: &State, mut rng: &mut R) -> Transaction {
    use rand::seq::IteratorRandom;
    let input_account = state.balances.keys().choose(&mut rng).unwrap();
    let output_account = state.balances.keys().choose(&mut rng).unwrap();
    let amount = rng.gen_range(0, state.balances[input_account]);
    Transaction {
        add: Addition {
            account: output_account.to_string(),
            amount,
        },
        sub: Subtraction {
            account: input_account.to_string(),
            amount,
        },
        nonce: rng.gen(),
    }
}

/// # Arguments
///
/// * `nodes_to_fail` - a set of nodes to be failed, i.e., nodes whose
/// phaselocks will never get unpaused, and a boolean indicating whether
/// to fail the first or last `num_failed_nodes` nodes.
async fn fail_nodes(
    num_nodes: u64,
    nodes_to_fail: HashSet<u64>,
    num_txns: u64,
    updated_timeout_ratio: Option<(u64, u64)>,
) -> Result<(), ConsensusError> {
    debug!("Number of nodes: {} ", num_nodes);

    // Calculate the threshold
    let threshold = get_threshold(num_nodes);

    // Generate the private key set
    let mut rng = Xoshiro256StarStar::seed_from_u64(SEED);
    let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut rng);

    // Get networking information
    let networkings =
        get_networkings::<Message<DEntryBlock, Transaction, State, H_256>>(num_nodes, &sks).await;
    debug!("All nodes connected to network");

    // Initialize the state and phaselocks
    let (mut state, mut phaselocks) = init_state_and_phaselocks(
        &sks,
        num_nodes,
        nodes_to_fail.clone(),
        threshold,
        networkings,
        updated_timeout_ratio,
    )
    .await;

    // Start phaselocks
    for phaselock in phaselocks.clone() {
        phaselock.start().await;
    }

    // Run random transactions
    let mut round = 1;
    let mut completed_txns = 0;
    let mut pending_txn: Option<Transaction> = None;
    let mut timed_out_views = 0;
    debug!("Running {} transactions", num_txns);
    while completed_txns < num_txns {
        println!("Round {}:", round);
        // The first node proposes a random transaction if there's no pending transaction
        let txn = match pending_txn.clone() {
            Some(t) => {
                if timed_out_views == num_nodes {
                    return Err(ConsensusError::TimedOutWithAnyLeader);
                }
                t
            }
            None => {
                let t = random_transaction(&state, &mut rng);
                debug!("Proposing: {:?}", t);
                if phaselocks[0].submit_transaction(t.clone()).await.is_err() {
                    return Err(ConsensusError::FailedToProposeTxn);
                }
                println!("Transaction {} proposed", completed_txns + 1);
                t
            }
        };

        // Start consensus
        let mut blocks = Vec::new();
        let mut states = Vec::new();
        let mut timed_out = false;
        for phaselock in &mut phaselocks {
            debug!("Waiting for consensus to occur");
            let mut event: Event<DEntryBlock, State> = match phaselock.next_event().await {
                Ok(event) => event,
                Err(err) => {
                    return Err(ConsensusError::PhaselockClosed(err));
                }
            };
            // Skip all messages from previous rounds
            while event.view_number < round {
                event = match phaselock.next_event().await {
                    Ok(event) => event,
                    Err(err) => {
                        error!(?err, "Error getting next event");
                        return Err(ConsensusError::PhaselockClosed(err));
                    }
                };
            }
            while !matches!(event.event, EventType::Decide { .. }) {
                if matches!(event.event, EventType::ViewTimeout { .. }) {
                    warn!(?event, "Round timed out!");
                    timed_out = true;
                    break;
                }
                event = match phaselock.next_event().await {
                    Ok(event) => event,
                    Err(err) => {
                        error!(?err, "Error getting next event");
                        return Err(ConsensusError::PhaselockClosed(err));
                    }
                };
            }
            if timed_out {
                pending_txn = Some(txn.clone());
                break;
            } else {
                pending_txn = None;
            }
            debug!("Decision emitted");
            if let EventType::Decide { block, state } = event.event {
                blocks.push(block);
                states.push(state);
            } else {
                unreachable!()
            }
        }
        if timed_out {
            timed_out_views += 1;
        } else {
            debug!("All nodes reached decision");

            // Check consensus
            assert!(states.len() as u64 == num_nodes - nodes_to_fail.len() as u64);
            assert!(blocks.len() as u64 == num_nodes - nodes_to_fail.len() as u64);
            let b_test = &blocks[0][0];
            for b in &blocks[1..] {
                if &b[0] != b_test {
                    return Err(ConsensusError::InconsistentAfterTxn);
                }
            }
            let s_test = &states[0][0];
            for s in &states[1..] {
                if &s[0] != s_test {
                    return Err(ConsensusError::InconsistentAfterTxn);
                }
            }
            println!("All states match");
            assert_eq!(blocks[0][0].transactions.len(), 1);
            assert_eq!(blocks[0][0].transactions, vec![txn.clone()]);
            state = s_test.clone();

            completed_txns += 1;
            timed_out_views = 0;
        }

        // Increment the round count
        round += 1;
    }

    println!("All rounds completed\n");
    Ok(())
}

async fn mul_txns(
    num_nodes: u64,
    txn_proposer_1: u64,
    txn_proposer_2: u64,
    updated_timeout_ratio: Option<(u64, u64)>,
) -> Result<(), ConsensusError> {
    debug!("Number of nodes: {} ", num_nodes);

    // Calculate the threshold
    let threshold = get_threshold(num_nodes);

    // Generate the private key set
    let mut rng = Xoshiro256StarStar::seed_from_u64(SEED);
    let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut rng);

    // Get networking information
    let networkings =
        get_networkings::<Message<DEntryBlock, Transaction, State, H_256>>(num_nodes, &sks).await;
    debug!("All nodes connected to network");

    // Initialize the state and phaselocks
    let (state, mut phaselocks) = init_state_and_phaselocks(
        &sks,
        num_nodes,
        HashSet::new(),
        threshold,
        networkings,
        updated_timeout_ratio,
    )
    .await;

    // Start phaselocks
    for phaselock in phaselocks.clone() {
        phaselock.start().await;
    }

    // Two nodes propose transactions
    debug!("Proposing two transactions");
    let txn_1 = random_transaction(&state, &mut rng);
    let txn_2 = random_transaction(&state, &mut rng);
    debug!("Txn 1: {:?}\n Txn 2: {:?}", txn_1, txn_2);
    if phaselocks[txn_proposer_1 as usize]
        .submit_transaction(txn_1.clone())
        .await
        .is_err()
    {
        return Err(ConsensusError::FailedToProposeTxn);
    }
    if phaselocks[txn_proposer_2 as usize]
        .submit_transaction(txn_2.clone())
        .await
        .is_err()
    {
        return Err(ConsensusError::FailedToProposeTxn);
    }
    debug!("Transactions proposed");

    // Start consensus
    let mut round: u64 = 1;
    loop {
        println!("Round {}:", round);
        if round > num_nodes {
            return Err(ConsensusError::TimedOutWithAnyLeader);
        }

        let mut blocks = Vec::new();
        let mut states = Vec::new();
        let mut timed_out = false;
        for phaselock in &mut phaselocks {
            debug!("Waiting for consensus to occur");
            let mut event: Event<DEntryBlock, State> = match phaselock.next_event().await {
                Ok(event) => event,
                Err(err) => {
                    return Err(ConsensusError::PhaselockClosed(err));
                }
            };
            // Skip all messages from previous rounds
            while event.view_number < round {
                event = match phaselock.next_event().await {
                    Ok(event) => event,
                    Err(err) => {
                        return Err(ConsensusError::PhaselockClosed(err));
                    }
                };
            }
            while !matches!(event.event, EventType::Decide { .. }) {
                if matches!(event.event, EventType::ViewTimeout { .. }) {
                    warn!(?event, "Round timed out!");
                    timed_out = true;
                    break;
                }
                event = match phaselock.next_event().await {
                    Ok(event) => event,
                    Err(err) => {
                        return Err(ConsensusError::PhaselockClosed(err));
                    }
                };
            }
            if timed_out {
                break;
            }
            debug!("Decision emitted");
            if let EventType::Decide { block, state } = event.event {
                blocks.push(block);
                states.push(state);
            } else {
                unreachable!()
            }
        }
        if !timed_out {
            debug!("All nodes reached decision");

            // Check consensus
            assert!(states.len() as u64 == num_nodes);
            assert!(blocks.len() as u64 == num_nodes);
            let b_test = &blocks[0][0];
            for b in &blocks[1..] {
                if &b[0] != b_test {
                    return Err(ConsensusError::InconsistentAfterTxn);
                }
            }
            let s_test = &states[0][0];
            for s in &states[1..] {
                if &s[0] != s_test {
                    return Err(ConsensusError::InconsistentAfterTxn);
                }
            }
            println!("All states match");
            assert!(!blocks[0][0].transactions.is_empty());
            assert_eq!(
                blocks[0][0].transactions,
                vec![txn_1.clone(), txn_2.clone()]
            );
            break;
        }

        // Increment the round count
        round += 1;
    }

    println!("Consensus completed\n");
    Ok(())
}

// Notes: Tests with #[ignore] are skipped because they fail nondeterministically due to timeout or config setting.

// TODO: Consensus behaves nondeterministically (https://gitlab.com/translucence/systems/hotstuff/-/issues/32)
#[ignore]
#[async_std::test]
async fn test_large_num_nodes_regression() {
    fail_nodes(50, HashSet::new(), 1, None)
        .await
        .unwrap_or_else(|err| panic!("{:?}", err));
    fail_nodes(90, HashSet::new(), 1, None)
        .await
        .unwrap_or_else(|err| panic!("{:?}", err));
}

#[ignore]
#[async_std::test]
async fn test_large_num_txns_regression() {
    fail_nodes(10, HashSet::new(), 11, Some((25, 10)))
        .await
        .unwrap_or_else(|err| panic!("{:?}", err));
}

#[async_std::test]
async fn test_fail_last_node_regression() {
    let mut nodes_to_fail = HashSet::new();
    nodes_to_fail.insert(52);
    fail_nodes(53, nodes_to_fail, 1, None)
        .await
        .unwrap_or_else(|err| panic!("{:?}", err));
}

#[async_std::test]
async fn test_fail_first_node_regression() {
    let mut nodes_to_fail = HashSet::new();
    nodes_to_fail.insert(0);
    fail_nodes(76, nodes_to_fail, 1, Some((25, 10)))
        .await
        .unwrap_or_else(|err| panic!("{:?}", err));
}

// TODO (issue): https://gitlab.com/translucence/systems/hotstuff/-/issues/31
#[ignore]
#[async_std::test]
async fn test_fail_last_f_nodes_regression() {
    let nodes_to_fail = HashSet::<u64>::from_iter((0..get_tolerance(75)).map(|x| 74 - x));
    fail_nodes(75, nodes_to_fail, 1, Some((11, 10)))
        .await
        .unwrap_or_else(|err| panic!("{:?}", err));
}

#[async_std::test]
async fn test_fail_last_f_plus_one_nodes_regression() {
    let nodes_to_fail = HashSet::<u64>::from_iter((0..get_tolerance(15) + 1).map(|x| 14 - x));
    match fail_nodes(15, nodes_to_fail, 1, Some((11, 10))).await {
        Err(ConsensusError::TimedOutWithAnyLeader) => {}
        _ => {
            panic!("Expected ConsensusError::TimedOutWithAnyLeader");
        }
    };
}

#[async_std::test]
async fn test_mul_txns_regression() {
    mul_txns(30, 5, 7, Some((20, 10)))
        .await
        .unwrap_or_else(|err| panic!("{:?}", err));
}

proptest! {
    #![proptest_config(ProptestConfig {
        timeout: 300000,
        cases: 10,
        .. ProptestConfig::default()
    })]
    // TODO: Consensus behaves nondeterministically (https://gitlab.com/translucence/systems/hotstuff/-/issues/32)
    #[ignore]
    #[test]
    fn test_large_num_nodes_random(num_nodes in 50..100u64) {
        async_std::task::block_on(
            async {
                fail_nodes(num_nodes, HashSet::new(), 1, None).await.unwrap_or_else(|err| {panic!("{:?}", err)});
            }
        );
    }

    // TODO: Consensus behaves nondeterministically (https://gitlab.com/translucence/systems/hotstuff/-/issues/32)
    #[ignore]
    #[test]
    fn test_large_num_txns_random(num_nodes in 5..30u64, num_txns in 10..30u64) {
        async_std::task::block_on(
            async {
                fail_nodes(num_nodes, HashSet::new(), num_txns, Some((25, 10))).await.unwrap_or_else(|err| {panic!("{:?}", err)});
            }
        );
    }

    // TODO: Consensus behaves nondeterministically (https://gitlab.com/translucence/systems/hotstuff/-/issues/32)
    #[ignore]
    #[test]
    fn test_fail_last_node_random(num_nodes in 30..100u64) {
        async_std::task::block_on(
            async {
                let mut nodes_to_fail = HashSet::new();
                nodes_to_fail.insert(num_nodes - 1);
                fail_nodes(num_nodes, nodes_to_fail, 1, None).await.unwrap_or_else(|err| {panic!("{:?}", err)});
            }
        );
    }

    // TODO: Consensus behaves nondeterministically (https://gitlab.com/translucence/systems/hotstuff/-/issues/32)
    #[ignore]
    #[test]
    fn test_fail_first_node_random(num_nodes in 30..100u64) {
        async_std::task::block_on(
            async {
                let mut nodes_to_fail = HashSet::new();
                nodes_to_fail.insert(0);
                fail_nodes(num_nodes, nodes_to_fail, 1, None).await.unwrap_or_else(|err| {panic!("{:?}", err)});
            }
        );
    }

    // TODO: Consensus times out with f failing nodes (https://gitlab.com/translucence/systems/hotstuff/-/issues/31)
    #[ignore]
    #[test]
    fn test_fail_last_f_nodes_random(num_nodes in 30..100u64) {
        async_std::task::block_on(
            async {
                let nodes_to_fail = HashSet::<u64>::from_iter((0..get_tolerance(num_nodes)).map(|x| num_nodes - x - 1));
                fail_nodes(num_nodes, nodes_to_fail, 5, Some((11, 10))).await.unwrap_or_else(|err| {panic!("{:?}", err)});
            }
        );
    }

    // TODO: Consensus times out with f failing nodes (https://gitlab.com/translucence/systems/hotstuff/-/issues/31)
    #[ignore]
    #[test]
    fn test_fail_first_f_nodes_random(num_nodes in 30..100u64) {
        async_std::task::block_on(
            async {
                let nodes_to_fail = HashSet::<u64>::from_iter(0..get_tolerance(num_nodes));
                fail_nodes(num_nodes, nodes_to_fail, 5, None).await.unwrap_or_else(|err| {panic!("{:?}", err)});
            }
        );
    }

    #[test]
    fn test_mul_txns_random(txn_proposer_1 in 0..15u64, txn_proposer_2 in 15..30u64) {
        async_std::task::block_on(
            async {
                mul_txns(30, txn_proposer_1, txn_proposer_2, Some((20, 10))).await.unwrap_or_else(|err| {panic!("{:?}", err)});
            }
        );
    }
}
