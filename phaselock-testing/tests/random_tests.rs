#![allow(clippy::type_complexity)]

mod common;

use async_std::prelude::FutureExt;
use async_std::task::sleep;
use common::{get_networkings, get_threshold, get_tolerance, init_state_and_phaselocks};
use phaselock::{
    demos::dentry::*,
    tc,
    traits::{implementations::MemoryNetwork, NodeImplementation, Storage},
    types::{Event, EventType, Message, PhaseLockHandle},
    PhaseLockConfig, PhaseLockError, PubKey, H_256,
};
use phaselock_testing::{ConsensusTestError, TestLauncher, TransactionSnafu};
use proptest::prelude::*;

use std::{collections::HashSet, iter::FromIterator};
use Either::{Left, Right};

// Notes: Tests with #[ignore] are skipped because they fail nondeterministically due to timeout or config setting.

// TODO: Consensus behaves nondeterministically (https://gitlab.com/translucence/systems/hotstuff/-/issues/32)
#[ignore]
#[async_std::test]
async fn test_large_num_nodes_regression() {
    let description_1 = TestDescription {
        total_nodes: 50,
        ..TestDescription::default()
    };
    run_rounds(description_1).await.unwrap();
    let description_2 = TestDescription {
        total_nodes: 90,
        ..TestDescription::default()
    };
    run_rounds(description_2).await.unwrap();
}

// fail_nodes(num_nodes, failures, num_txns, timeout_ratio)

#[ignore]
#[async_std::test]
async fn test_large_num_txns_regression() {
    let description = TestDescription {
        total_nodes: 10,
        txn_ids: Right((11, 1)),
        timeout_ratio: (25, 10),
        ..TestDescription::default()
    };
    run_rounds(description).await.unwrap();
}

#[async_std::test]
async fn test_fail_last_node_regression() {
    let description = TestDescription {
        total_nodes: 53,
        ids_to_shut_down: vec![52].into_iter().collect::<HashSet<_>>(),
        ..TestDescription::default()
    };
    run_rounds(description).await.unwrap();
}

#[async_std::test]
async fn test_fail_first_node_regression() {
    let description = TestDescription {
        total_nodes: 76,
        ids_to_shut_down: vec![0].into_iter().collect::<HashSet<_>>(),
        timeout_ratio: (25, 10),
        ..TestDescription::default()
    };
    run_rounds(description).await.unwrap();
}

// TODO (issue): https://gitlab.com/translucence/systems/hotstuff/-/issues/31
#[ignore]
#[async_std::test]
async fn test_fail_last_f_nodes_regression() {
    let description = TestDescription {
        total_nodes: 75,
        ids_to_shut_down: HashSet::<u64>::from_iter((0..get_tolerance(75)).map(|x| 74 - x)),
        ..TestDescription::default()
    };
    run_rounds(description).await.unwrap();
}

#[async_std::test]
async fn test_fail_last_f_plus_one_nodes_regression() {
    let description = TestDescription {
        total_nodes: 15,
        ids_to_shut_down: HashSet::<u64>::from_iter((0..get_tolerance(15) + 1).map(|x| 14 - x)),
        ..TestDescription::default()
    };
    run_rounds(description).await.unwrap();
}

// TODO (vko): these tests seem to fail in CI
#[ignore]
#[async_std::test]
async fn test_mul_txns_regression() {
    let description = TestDescription {
        total_nodes: 30,
        ..TestDescription::default()
    };
    run_rounds(description).await.unwrap();
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
    fn test_large_num_nodes_random(num_nodes in 50..100usize) {
        let description = TestDescription {
            total_nodes: num_nodes,
            ..TestDescription::default()
        };
        async_std::task::block_on(
            async {
                run_rounds(description).await.unwrap();
            }
        );
    }

    // TODO: Consensus behaves nondeterministically (https://gitlab.com/translucence/systems/hotstuff/-/issues/32)
    #[ignore]
    #[test]
    fn test_large_num_txns_random(num_nodes in 5..30usize, num_txns in 10..30usize) {
        let description = TestDescription {
            total_nodes: num_nodes,
            txn_ids: Right((num_txns, 1)),
            timeout_ratio: (25, 10),
            ..TestDescription::default()
        };
        async_std::task::block_on(
            async {
                run_rounds(description).await.unwrap();
            }
        );
    }

    // TODO: Consensus behaves nondeterministically (https://gitlab.com/translucence/systems/hotstuff/-/issues/32)
    #[ignore]
    #[test]
    fn test_fail_last_node_random(num_nodes in 30..100usize) {
        let description = TestDescription {
            total_nodes: num_nodes,
            ids_to_shut_down: vec![(num_nodes - 1) as u64].into_iter().collect(),
            ..TestDescription::default()
        };
        async_std::task::block_on(
            async {
                run_rounds(description).await.unwrap();
            }
        );
    }

    // TODO: Consensus behaves nondeterministically (https://gitlab.com/translucence/systems/hotstuff/-/issues/32)
    #[ignore]
    #[test]
    fn test_fail_first_node_random(num_nodes in 30..100usize) {
        let description = TestDescription {
            total_nodes: num_nodes,
            ids_to_shut_down: vec![0].into_iter().collect(),
            ..TestDescription::default()
        };
        async_std::task::block_on(
            async {
                run_rounds(description).await.unwrap();
            }
        );
    }

    // TODO: Consensus times out with f failing nodes (https://gitlab.com/translucence/systems/hotstuff/-/issues/31)
    #[ignore]
    #[test]
    fn test_fail_last_f_nodes_random(num_nodes in 30..100usize) {
        let description = TestDescription {
            total_nodes: num_nodes,
            ids_to_shut_down: HashSet::<u64>::from_iter((0..get_tolerance(num_nodes as u64)).map(|x| (num_nodes as u64) - x - 1)),
            txn_ids: Right((5, 1)),
            ..TestDescription::default()
        };
        async_std::task::block_on(
            async {
                run_rounds(description).await.unwrap();
            }
        );
    }

    // TODO: Consensus times out with f failing nodes (https://gitlab.com/translucence/systems/hotstuff/-/issues/31)
    #[ignore]
    #[test]
    fn test_fail_first_f_nodes_random(num_nodes in 30..100usize) {
        let description = TestDescription {
            total_nodes: num_nodes,
            ids_to_shut_down: HashSet::<u64>::from_iter(0..get_tolerance(num_nodes as u64)),
            txn_ids: Right((5, 1)),
            ..TestDescription::default()
        };
        async_std::task::block_on(
            async {
                run_rounds(description).await.unwrap();
            }
        );
    }

    // TODO (vko): these tests seem to fail in CI
    #[ignore]
    #[test]
    fn test_mul_txns_random(txn_proposer_1 in 0..15u64, txn_proposer_2 in 15..30u64) {
        let description = TestDescription {
            total_nodes: 30,
            txn_ids: Left(vec![vec![txn_proposer_1, txn_proposer_2]]),
            ..TestDescription::default()
        };
        async_std::task::block_on(
            async {
                run_rounds(description).await.unwrap();
            }
        );
    }
}

#[async_std::test]
pub async fn test_harness() {
    let mut runner = TestLauncher::new(30).launch();

    runner.add_nodes(30).await;
    for node in runner.nodes() {
        let qc = node.storage().get_newest_qc().await.unwrap().unwrap();
        assert_eq!(qc.view_number, 0);
    }
    runner
        .add_random_transaction(None)
        .expect("Could not add a random transaction");
    let _ = runner.run_one_round().await;
    for node in runner.nodes() {
        let qc = node.storage().get_newest_qc().await.unwrap().unwrap();
        assert_eq!(qc.view_number, 1);
    }

    runner.shutdown_all().await;
}
