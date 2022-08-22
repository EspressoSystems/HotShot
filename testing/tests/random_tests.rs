#![allow(clippy::type_complexity)]

mod common;

#[cfg(feature = "slow-tests")]
use common::{get_tolerance, GeneralTestDescriptionBuilder, TestDescription};
#[cfg(feature = "slow-tests")]
use either::Either::{Left, Right};
#[cfg(feature = "slow-tests")]
use hotshot::{
    demos::dentry::{DEntryBlock, State},
    traits::implementations::{AtomicStorage, Libp2pNetwork, MemoryNetwork, MemoryStorage},
};
#[cfg(feature = "slow-tests")]
use std::{collections::HashSet, iter::FromIterator};

cross_all_types!(
    test_fifty_nodes_regression,
    GeneralTestDescriptionBuilder {
        total_nodes: 50,
        start_nodes: 50,
        num_succeeds: 40,
        txn_ids: Right(30),
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true
);

cross_all_types!(
    test_ninety_nodes_regression,
    GeneralTestDescriptionBuilder {
        total_nodes: 90,
        start_nodes: 90,
        num_succeeds: 40,
        txn_ids: Right(30),
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true
);

cross_all_types!(
    test_large_num_txns_regression,
    GeneralTestDescriptionBuilder {
        total_nodes: 10,
        start_nodes: 10,
        num_succeeds: 40,
        txn_ids: Right(500),
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true
);

cross_all_types!(
    test_fail_last_node_regression,
    GeneralTestDescriptionBuilder {
        total_nodes: 53,
        start_nodes: 53,
        num_succeeds: 40,
        txn_ids: Right(30),
        ids_to_shut_down: vec![vec![52].into_iter().collect::<HashSet<_>>()],
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true
);

cross_all_types!(
    test_fail_first_node_regression,
    GeneralTestDescriptionBuilder {
        total_nodes: 76,
        start_nodes: 76,
        num_succeeds: 40,
        txn_ids: Right(30),
        ids_to_shut_down: vec![vec![0].into_iter().collect::<HashSet<_>>()],
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true
);

cross_all_types!(
    test_fail_last_f_nodes_regression,
    GeneralTestDescriptionBuilder {
        total_nodes: 75,
        start_nodes: 75,
        num_succeeds: 40,
        txn_ids: Right(30),
        ids_to_shut_down: vec![HashSet::<u64>::from_iter(
            (0..get_tolerance(75)).map(|x| 74 - x),
        )],
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true
);

cross_all_types!(
    test_fail_last_f_plus_one_nodes_regression,
    GeneralTestDescriptionBuilder {
        total_nodes: 15,
        start_nodes: 15,
        txn_ids: Right(30),
        ids_to_shut_down: vec![HashSet::<u64>::from_iter(
            (0..get_tolerance(15) + 1).map(|x| 14 - x),
        )],
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true
);

cross_all_types!(
    test_mul_txns_regression,
    GeneralTestDescriptionBuilder {
        total_nodes: 30,
        start_nodes: 30,
        txn_ids: Right(30),
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true
);

cross_all_types_proptest!(
    test_large_num_nodes_random,
    GeneralTestDescriptionBuilder {
        total_nodes: num_nodes,
        start_nodes: num_nodes,
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true,
    args: num_nodes in 50..100usize
);

cross_all_types_proptest!(
    test_fail_last_node_random,
    GeneralTestDescriptionBuilder {
        total_nodes: num_nodes,
        start_nodes: num_nodes,
        txn_ids: Right(30),
        ids_to_shut_down: vec![vec![(num_nodes - 1) as u64].into_iter().collect()],
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true,
    args: num_nodes in 30..100usize
);

cross_all_types_proptest!(
    test_fail_first_node_random,
    GeneralTestDescriptionBuilder {
        total_nodes: num_nodes,
        start_nodes: num_nodes,
        txn_ids: Right(30),
        ids_to_shut_down: vec![vec![0].into_iter().collect()],
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true,
    args: num_nodes in 30..100usize
);

cross_all_types_proptest!(
    test_fail_last_f_nodes_random,
    GeneralTestDescriptionBuilder {
        total_nodes: num_nodes,
        start_nodes: num_nodes,
        num_succeeds: 40,
        txn_ids: Right(30),
        ids_to_shut_down: vec![HashSet::<u64>::from_iter((0..get_tolerance(num_nodes as u64)).map(|x| (num_nodes as u64) - x - 1))],
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true,
    args: num_nodes in 30..100usize
);

cross_all_types_proptest!(
    test_fail_first_f_nodes_random,
    GeneralTestDescriptionBuilder {
        total_nodes: num_nodes,
        start_nodes: num_nodes,
        num_succeeds: 40,
        txn_ids: Right(30),
        ids_to_shut_down: vec![HashSet::<u64>::from_iter(0..get_tolerance(num_nodes as u64))],
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true,
    args: num_nodes in 30..100usize
);

cross_all_types_proptest!(
    test_mul_txns_random,
    GeneralTestDescriptionBuilder {
        total_nodes: 30,
        start_nodes: 30,
        txn_ids: Left(vec![vec![txn_proposer_1, txn_proposer_2]]),
        ..GeneralTestDescriptionBuilder::default()
    },
    keep: true,
    slow: true,
    args: txn_proposer_1 in 0..15u64, txn_proposer_2 in 15..30u64
);

// TODO rewrite this test to not use storage?
// or, nuke.
// #[async_std::test]
// #[ignore]
// pub async fn test_harness() {
//     let run_round = |runner: &mut AppliedTestRunner| -> Vec<TestTransaction> {
//         runner
//             .add_random_transactions(2)
//             .expect("Could not add a random transaction")
//     };
//
//     let safety_check_pre = |runner: &AppliedTestRunner| -> Result<(), ConsensusRoundError> {
//         block_on(async move {
//             for node in runner.nodes() {
//                 let qc = node.storage().get_newest_qc().await.unwrap().unwrap();
//                 assert_eq!(qc.view_number, ViewNumber::new(0));
//             }
//         });
//         Ok(())
//     };
//
//     let safety_check_post = |runner: &AppliedTestRunner, _results: TestRoundResult| {
//         block_on(async move {
//             for node in runner.nodes() {
//                 let qc = node.storage().get_newest_qc().await.unwrap().unwrap();
//                 assert_eq!(qc.view_number, ViewNumber::new(1));
//             }
//         });
//         Ok(())
//     };
//
//     let test_description = DetailedTestDescriptionBuilder {
//         rounds: Some(vec![Round {
//             safety_check_post: Some(Arc::new(safety_check_post)),
//             setup_round: Some(Arc::new(run_round)),
//             safety_check_pre: Some(Arc::new(safety_check_pre)),
//         }]),
//         general_info: GeneralTestDescriptionBuilder {
//             total_nodes: 30,
//             start_nodes: 30,
//             ..GeneralTestDescriptionBuilder::default()
//         },
//         gen_runner: None,
//     };
//
//     test_description.build().execute().await.unwrap();
// }
