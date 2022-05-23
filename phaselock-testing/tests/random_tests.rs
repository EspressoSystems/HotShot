#![allow(clippy::type_complexity)]

mod common;

use async_std::task::block_on;
use common::{get_tolerance, AppliedTestRunner, TestNetwork, TestRoundResult, TestTransaction};
use phaselock::traits::Storage;
use phaselock_testing::{ConsensusRoundError, Round};
use phaselock_types::data::ViewNumber;
use proptest::prelude::*;
use tracing::instrument;

use crate::common::TestDescriptionBuilder;
use either::Either::{Left, Right};
use std::{collections::HashSet, iter::FromIterator, sync::Arc};

// Notes: Tests with #[ignore] are skipped because they fail nondeterministically due to timeout or config setting.

// TODO: Consensus behaves nondeterministically (https://github.com/EspressoSystems/phaselock/issues/15)
#[ignore]
#[async_std::test]
#[instrument]
async fn test_large_num_nodes_regression() {
    let description_1 = TestDescriptionBuilder::<TestNetwork, _> {
        total_nodes: 50,
        start_nodes: 50,
        ..TestDescriptionBuilder::default()
    };
    description_1.build().execute().await.unwrap();
    let description_2 = TestDescriptionBuilder::<TestNetwork, _> {
        total_nodes: 90,
        ..TestDescriptionBuilder::default()
    };
    description_2.build().execute().await.unwrap();
}

#[async_std::test]
#[instrument]
async fn test_large_num_txns_regression() {
    let description = TestDescriptionBuilder::<TestNetwork, _> {
        total_nodes: 10,
        start_nodes: 10,
        num_succeeds: 11,
        txn_ids: Right(1),
        timeout_ratio: (25, 10),
        ..TestDescriptionBuilder::default()
    };
    description.build().execute().await.unwrap();
}

// TODO jr: fix failure
#[async_std::test]
#[instrument]
#[ignore]
async fn test_fail_last_node_regression() {
    let description = TestDescriptionBuilder::<TestNetwork, _> {
        total_nodes: 53,
        start_nodes: 53,
        next_view_timeout: 1000,
        ids_to_shut_down: vec![vec![52].into_iter().collect::<HashSet<_>>()],
        ..TestDescriptionBuilder::default()
    };
    description.build().execute().await.unwrap();
}

// TODO jr: fix failure
#[async_std::test]
#[instrument]
#[ignore]
async fn test_fail_first_node_regression() {
    let description = TestDescriptionBuilder::<TestNetwork, _> {
        total_nodes: 76,
        start_nodes: 76,
        ids_to_shut_down: vec![vec![0].into_iter().collect::<HashSet<_>>()],
        next_view_timeout: 1000,
        timeout_ratio: (25, 10),
        ..TestDescriptionBuilder::default()
    };
    description.build().execute().await.unwrap();
}

// TODO (issue): https://github.com/EspressoSystems/phaselock/issues/15
#[ignore]
#[async_std::test]
#[instrument]
async fn test_fail_last_f_nodes_regression() {
    let description = TestDescriptionBuilder::<TestNetwork, _> {
        total_nodes: 75,
        start_nodes: 75,
        next_view_timeout: 1000,
        ids_to_shut_down: vec![HashSet::<u64>::from_iter(
            (0..get_tolerance(75)).map(|x| 74 - x),
        )],
        ..TestDescriptionBuilder::default()
    };
    description.build().execute().await.unwrap();
}

// TODO jr: fix failure
#[async_std::test]
#[instrument]
#[ignore]
async fn test_fail_last_f_plus_one_nodes_regression() {
    let description = TestDescriptionBuilder::<TestNetwork, _> {
        total_nodes: 15,
        start_nodes: 15,
        next_view_timeout: 1000,
        ids_to_shut_down: vec![HashSet::<u64>::from_iter(
            (0..get_tolerance(15) + 1).map(|x| 14 - x),
        )],
        ..TestDescriptionBuilder::default()
    };
    description.build().execute().await.unwrap();
}

// TODO (vko): these tests seem to fail in CI
// #[ignore]
#[async_std::test]
#[instrument]
async fn test_mul_txns_regression() {
    let description = TestDescriptionBuilder::<TestNetwork, _> {
        total_nodes: 30,
        start_nodes: 30,
        ..TestDescriptionBuilder::default()
    };
    description.build().execute().await.unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig {
        timeout: 300000,
        cases: 10,
        .. ProptestConfig::default()
    })]
    // TODO: Consensus behaves nondeterministically (https://github.com/EspressoSystems/phaselock/issues/15)
    #[ignore]
    #[test]
    fn test_large_num_nodes_random(num_nodes in 50..100usize) {
        let description = TestDescriptionBuilder::<TestNetwork, _> {
            total_nodes: num_nodes,
            start_nodes: num_nodes,
            ..TestDescriptionBuilder::default()
        };
        async_std::task::block_on(
            async {
                description.build().execute().await.unwrap();
            }
        );
    }

    // TODO: Consensus behaves nondeterministically (https://github.com/EspressoSystems/phaselock/issues/15)
    #[ignore]
    #[test]
    fn test_large_num_txns_random(num_nodes in 5..30usize, num_txns in 10..30usize) {
        let description = TestDescriptionBuilder::<TestNetwork, _> {
            total_nodes: num_nodes,
            start_nodes: num_nodes,
            num_succeeds: num_txns,
            txn_ids: Right(1),
            timeout_ratio: (25, 10),
            ..TestDescriptionBuilder::default()
        };
        async_std::task::block_on(
            async {
                description.build().execute().await.unwrap();
            }
        );
    }

    // TODO: Consensus behaves nondeterministically (https://github.com/EspressoSystems/phaselock/issues/15)
    #[ignore]
    #[test]
    fn test_fail_last_node_random(num_nodes in 30..100usize) {
        let description = TestDescriptionBuilder::<TestNetwork, _> {
            total_nodes: num_nodes,
            start_nodes: num_nodes,
            ids_to_shut_down: vec![vec![(num_nodes - 1) as u64].into_iter().collect()],
            ..TestDescriptionBuilder::default()
        };
        async_std::task::block_on(
            async {
                description.build().execute().await.unwrap();
            }
        );
    }

    // TODO: Consensus behaves nondeterministically (https://github.com/EspressoSystems/phaselock/issues/15)
    #[ignore]
    #[test]
    fn test_fail_first_node_random(num_nodes in 30..100usize) {
        let description = TestDescriptionBuilder::<TestNetwork, _> {
            total_nodes: num_nodes,
            start_nodes: num_nodes,
            ids_to_shut_down: vec![vec![0].into_iter().collect()],
            ..TestDescriptionBuilder::default()
        };
        async_std::task::block_on(
            async {
                description.build().execute().await.unwrap();
            }
        );
    }

    // TODO: Consensus times out with f failing nodes (https://github.com/EspressoSystems/phaselock/issues/15)
    #[ignore]
    #[test]
    fn test_fail_last_f_nodes_random(num_nodes in 30..100usize) {
        let description = TestDescriptionBuilder::<TestNetwork, _> {
            total_nodes: num_nodes,
            start_nodes: num_nodes,
            ids_to_shut_down: vec![HashSet::<u64>::from_iter((0..get_tolerance(num_nodes as u64)).map(|x| (num_nodes as u64) - x - 1))],
            num_succeeds: 5,
            txn_ids: Right(1),
            ..TestDescriptionBuilder::default()
        };
        async_std::task::block_on(
            async {
                description.build().execute().await.unwrap();
            }
        );
    }

    // TODO: Consensus times out with f failing nodes (https://github.com/EspressoSystems/phaselock/issues/15)
    #[ignore]
    #[test]
    fn test_fail_first_f_nodes_random(num_nodes in 30..100usize) {
        let description = TestDescriptionBuilder::<TestNetwork, _> {
            total_nodes: num_nodes,
            start_nodes: num_nodes,
            ids_to_shut_down: vec![HashSet::<u64>::from_iter(0..get_tolerance(num_nodes as u64))],
            num_succeeds: 5,
            txn_ids: Right(1),
            ..TestDescriptionBuilder::default()
        };
        async_std::task::block_on(
            async {
                description.build().execute().await.unwrap();
            }
        );
    }

    // TODO (vko): these tests seem to fail in CI
    #[ignore]
    #[test]
    fn test_mul_txns_random(txn_proposer_1 in 0..15u64, txn_proposer_2 in 15..30u64) {
        let description = TestDescriptionBuilder::<TestNetwork, _> {
            total_nodes: 30,
            start_nodes: 30,
            txn_ids: Left(vec![vec![txn_proposer_1, txn_proposer_2]]),
            ..TestDescriptionBuilder::default()
        };
        async_std::task::block_on(
            async {
                description.build().execute().await.unwrap();
            }
        );
    }
}

#[async_std::test]
pub async fn test_harness() {
    let run_round = |runner: &mut AppliedTestRunner| -> Vec<TestTransaction> {
        runner
            .add_random_transactions(2)
            .expect("Could not add a random transaction")
    };

    let safety_check_pre = |runner: &AppliedTestRunner| -> Result<(), ConsensusRoundError> {
        block_on(async move {
            for node in runner.nodes() {
                let qc = node.storage().get_newest_qc().await.unwrap().unwrap();
                assert_eq!(qc.view_number, ViewNumber::new(0));
            }
        });
        Ok(())
    };

    let safety_check_post = |runner: &AppliedTestRunner, _results: TestRoundResult| {
        block_on(async move {
            for node in runner.nodes() {
                let qc = node.storage().get_newest_qc().await.unwrap().unwrap();
                assert_eq!(qc.view_number, ViewNumber::new(1));
            }
        });
        Ok(())
    };

    let test_description = TestDescriptionBuilder {
        rounds: Some(vec![Round {
            safety_check_post: Some(Arc::new(safety_check_post)),
            setup_round: Some(Arc::new(run_round)),
            safety_check_pre: Some(Arc::new(safety_check_pre)),
        }]),
        total_nodes: 30,
        start_nodes: 30,
        ..TestDescriptionBuilder::default()
    };

    test_description.build().execute().await.unwrap();
}
