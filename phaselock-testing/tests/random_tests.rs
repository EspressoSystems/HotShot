#![allow(clippy::type_complexity)]

mod common;

use async_std::prelude::FutureExt;
use common::{
    get_networkings, get_threshold, get_tolerance, init_state_and_phaselocks, setup_logging,
};
use phaselock::{
    demos::dentry::*,
    tc,
    traits::{implementations::MemoryNetwork, NodeImplementation, Storage},
    types::{Event, EventType, Message, PhaseLockHandle},
    PhaseLockConfig, PhaseLockError, PubKey, H_256,
};
use phaselock_testing::{ConsensusTestError, TestLauncher, TransactionSnafu};
use proptest::prelude::*;
use rand::thread_rng;
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use snafu::ResultExt;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    iter::FromIterator,
    sync::Arc,
    time::Duration,
};
use tracing::{debug, error, warn};

use crate::common::setup_logging;

const NEXT_VIEW_TIMEOUT: u64 = 100;
const DEFAULT_TIMEOUT_RATIO: (u64, u64) = (15, 10);
const SEED: u64 = 1234;

#[allow(clippy::upper_case_acronyms)]
type NODE = DEntryNode<MemoryNetwork<Message<DEntryBlock, Transaction, State, H_256>>>;

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
) -> Result<(), ConsensusTestError> {
    todo!()
    // debug!("Number of nodes: {} ", num_nodes);
    //
    // // Calculate the threshold
    // let threshold = get_threshold(num_nodes);
    //
    // // Generate the private key set
    // let mut rng = Xoshiro256StarStar::seed_from_u64(SEED);
    // let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut rng);
    //
    // // Get networking information
    // let (_, networkings) =
    //     get_networkings::<Message<DEntryBlock, Transaction, State, H_256>>(num_nodes, &sks).await;
    // debug!("All nodes connected to network");
    //
    // // Initialize the state and phaselocks
    // let known_nodes: Vec<_> = (0..num_nodes)
    //     .map(|x| PubKey::from_secret_key_set_escape_hatch(&sks, x))
    //     .collect();
    // let (mut state, mut phaselocks) = init_state_and_phaselocks::<NODE, H_256>(
    //     &sks,
    //     num_nodes,
    //     known_nodes,
    //     nodes_to_fail.clone(),
    //     threshold,
    //     networkings,
    //     updated_timeout_ratio.unwrap_or(DEFAULT_TIMEOUT_RATIO),
    //     NEXT_VIEW_TIMEOUT,
    //     init_state(),
    // )
    // .await;
    //
    // // Start phaselocks
    // for phaselock in phaselocks.clone() {
    //     phaselock.start().await;
    // }
    //
    // // Run random transactions
    // let mut round = 1;
    // let mut completed_txns = 0;
    // let mut pending_txn: Option<Transaction> = None;
    // let mut timed_out_views = 0;
    // debug!("Running {} transactions", num_txns);
    // while completed_txns < num_txns {
    //     println!("Round {}:", round);
    //     // The first node proposes a random transaction if there's no pending transaction
    //     let txn = match pending_txn.clone() {
    //         Some(t) => {
    //             if timed_out_views == num_nodes {
    //                 return Err(ConsensusTestError::TimedOutWithAnyLeader);
    //             }
    //             t
    //         }
    //         None => {
    //             let t = random_transaction(&state, &mut rng);
    //             debug!("Proposing: {:?}", t);
    //             if phaselocks[0].submit_transaction(t.clone()).await.is_err() {
    //                 return Err(ConsensusTestError::FailedToProposeTxn);
    //             }
    //             println!("Transaction {} proposed", completed_txns + 1);
    //             t
    //         }
    //     };
    //
    //     // Start consensus
    //     let mut blocks = Vec::new();
    //     let mut states = Vec::new();
    //     let mut timed_out = false;
    //     for phaselock in &mut phaselocks {
    //         debug!("Waiting for consensus to occur");
    //         let mut event: Event<DEntryBlock, State> = match phaselock.next_event().await {
    //             Ok(event) => event,
    //             Err(err) => {
    //                 return Err(ConsensusTestError::PhaselockClosed(err));
    //             }
    //         };
    //         // Skip all messages from previous rounds
    //         while event.view_number < round {
    //             event = match phaselock.next_event().await {
    //                 Ok(event) => event,
    //                 Err(err) => {
    //                     error!(?err, "Error getting next event");
    //                     return Err(ConsensusTestError::PhaselockClosed(err));
    //                 }
    //             };
    //         }
    //         while !matches!(event.event, EventType::Decide { .. }) {
    //             if matches!(event.event, EventType::ViewTimeout { .. }) {
    //                 warn!(?event, "Round timed out!");
    //                 timed_out = true;
    //                 break;
    //             }
    //             event = match phaselock.next_event().await {
    //                 Ok(event) => event,
    //                 Err(err) => {
    //                     error!(?err, "Error getting next event");
    //                     return Err(ConsensusTestError::PhaselockClosed(err));
    //                 }
    //             };
    //         }
    //         if timed_out {
    //             pending_txn = Some(txn.clone());
    //             break;
    //         } else {
    //             pending_txn = None;
    //         }
    //         debug!("Decision emitted");
    //         if let EventType::Decide { block, state } = event.event {
    //             blocks.push(block);
    //             states.push(state);
    //         } else {
    //             unreachable!()
    //         }
    //     }
    //     if timed_out {
    //         timed_out_views += 1;
    //     } else {
    //         debug!("All nodes reached decision");
    //
    //         // Check consensus
    //         assert!(states.len() as u64 == num_nodes - nodes_to_fail.len() as u64);
    //         assert!(blocks.len() as u64 == num_nodes - nodes_to_fail.len() as u64);
    //         let b_test = &blocks[0][0];
    //         for b in &blocks[1..] {
    //             if &b[0] != b_test {
    //                 return Err(ConsensusTestError::InconsistentAfterTxn);
    //             }
    //         }
    //         let s_test = &states[0][0];
    //         for s in &states[1..] {
    //             if &s[0] != s_test {
    //                 return Err(ConsensusTestError::InconsistentAfterTxn);
    //             }
    //         }
    //         println!("All states match");
    //         assert_eq!(blocks[0][0].transactions.len(), 1);
    //         assert_eq!(blocks[0][0].transactions, vec![txn.clone()]);
    //         state = s_test.clone();
    //
    //         completed_txns += 1;
    //         timed_out_views = 0;
    //     }
    //
    //     // Increment the round count
    //     round += 1;
    // }
    //
    // println!("All rounds completed\n");
    // Ok(())
}

async fn mul_txns(
    total_nodes: usize,
    txn_ids: Vec<usize>,
    next_view_timeout: u64,
    timeout_ratio: (u64, u64),
    round_start_delay: u64,
    start_delay: u64,
) -> Result<(), ConsensusTestError> {
    // there must exists transactions
    assert!(txn_ids.len() > 0);

    setup_logging();

    let threshold = ((total_nodes * 2) / 3) + 1;
    let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut thread_rng());
    let known_nodes: Vec<PubKey> = (0..total_nodes)
        .map(|node_id| PubKey::from_secret_key_set_escape_hatch(&sks, node_id as u64))
        .collect();
    let default_config = PhaseLockConfig {
        total_nodes: total_nodes as u32,
        threshold: threshold as u32,
        max_transactions: 100,
        known_nodes,
        next_view_timeout,
        timeout_ratio,
        round_start_delay,
        start_delay,
    };
    let mut runner = TestLauncher::new(total_nodes)
        .with_default_config(default_config)
        .launch();
    runner.add_nodes(total_nodes).await;
    for node in runner.nodes() {
        let qc = node.storage().get_newest_qc().await.unwrap().unwrap();
        assert_eq!(qc.view_number, 0);
    }
    let txns = runner
        .add_random_transactions(2)
        .context(TransactionSnafu)?;

    // Start consensus
    // TODO this might error out if no transactions more transactions submitted?
    match runner.run_one_round().await {
        Ok((states, blocks)) => {
            debug!("All nodes reached decision");
            // Check consensus
            assert!(states.len() == total_nodes);
            assert!(blocks.len() == total_nodes);
            let b_test = &blocks[0][0];
            for b in &blocks[1..] {
                if &b[0] != b_test {
                    return Err(ConsensusTestError::InconsistentAfterTxn);
                }
            }
            let s_test = &states[0][0];
            for s in &states[1..] {
                if &s[0] != s_test {
                    return Err(ConsensusTestError::InconsistentAfterTxn);
                }
            }
            println!("All states match");
            assert!(!blocks[0][0].transactions.is_empty());
            assert_eq!(blocks[0][0].transactions, txns);
        }
        Err(e) => {
            panic!("failed round with {:?}", e);
        }
    }

    println!("Consensus completed\n");
    Ok(())
}

// Notes: Tests with #[ignore] are skipped because they fail nondeterministically due to timeout or config setting.

// TODO: Consensus behaves nondeterministically (https://gitlab.com/translucence/systems/hotstuff/-/issues/32)
// #[ignore]
// #[async_std::test]
// async fn test_large_num_nodes_regression() {
//     fail_nodes(50, HashSet::new(), 1, None)
//         .await
//         .unwrap_or_else(|err| panic!("{:?}", err));
//     fail_nodes(90, HashSet::new(), 1, None)
//         .await
//         .unwrap_or_else(|err| panic!("{:?}", err));
// }
//
// #[ignore]
// #[async_std::test]
// async fn test_large_num_txns_regression() {
//     fail_nodes(10, HashSet::new(), 11, Some((25, 10)))
//         .await
//         .unwrap_or_else(|err| panic!("{:?}", err));
// }
//
// #[async_std::test]
// async fn test_fail_last_node_regression() {
//     let mut nodes_to_fail = HashSet::new();
//     nodes_to_fail.insert(52);
//     fail_nodes(53, nodes_to_fail, 1, None)
//         .await
//         .unwrap_or_else(|err| panic!("{:?}", err));
// }
//
// #[async_std::test]
// async fn test_fail_first_node_regression() {
//     let mut nodes_to_fail = HashSet::new();
//     nodes_to_fail.insert(0);
//     fail_nodes(76, nodes_to_fail, 1, Some((25, 10)))
//         .await
//         .unwrap_or_else(|err| panic!("{:?}", err));
// }
//
// // TODO (issue): https://gitlab.com/translucence/systems/hotstuff/-/issues/31
// #[ignore]
// #[async_std::test]
// async fn test_fail_last_f_nodes_regression() {
//     let nodes_to_fail = HashSet::<u64>::from_iter((0..get_tolerance(75)).map(|x| 74 - x));
//     fail_nodes(75, nodes_to_fail, 1, Some((11, 10)))
//         .await
//         .unwrap_or_else(|err| panic!("{:?}", err));
// }
//
// #[async_std::test]
// async fn test_fail_last_f_plus_one_nodes_regression() {
//     let nodes_to_fail = HashSet::<u64>::from_iter((0..get_tolerance(15) + 1).map(|x| 14 - x));
//     match fail_nodes(15, nodes_to_fail, 1, Some((11, 10))).await {
//         Err(ConsensusTestError::TimedOutWithAnyLeader) => {}
//         _ => {
//             panic!("Expected ConsensusTestError::TimedOutWithAnyLeader");
//         }
//     };
// }

// TODO (vko): these tests seem to fail in CI
#[ignore]
#[async_std::test]
async fn test_mul_txns_regression() {
    mul_txns(30, vec![1, 2, 3, 4], 100, (20, 10), 1, 1)
        .await
        .unwrap()
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

    // TODO (vko): these tests seem to fail in CI
    #[ignore]
    #[test]
    fn test_mul_txns_random(txn_proposer_1 in 0..15usize, txn_proposer_2 in 15..30usize) {
        async_std::task::block_on(
            async {
                mul_txns(30, vec![txn_proposer_1, txn_proposer_2], 1000, (20, 10), 1, 1).await.unwrap()
            }
        );
    }
}

#[async_std::test]
pub async fn test_harness() {
    setup_logging();
    let mut runner = TestLauncher::new(5).launch();

    runner.add_nodes(5).await;
    for node in runner.nodes() {
        let qc = node.storage().get_newest_qc().await.unwrap().unwrap();
        assert_eq!(qc.view_number, 0);
    }
    runner
        .add_random_transaction(None)
        .expect("Could not add a random transaction");
    runner.run_one_round().await.unwrap();
    for node in runner.nodes() {
        let qc = node.storage().get_newest_qc().await.unwrap().unwrap();
        assert_eq!(qc.view_number, 1);
    }
}
