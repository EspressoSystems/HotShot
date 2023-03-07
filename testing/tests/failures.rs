// #![allow(clippy::type_complexity)]

// use hotshot_testing::cross_all_types;

// // This test simulates a single permanent failed node
// cross_all_types!(
//     single_permanent_failure,
//     hotshot_testing::test_description::GeneralTestDescriptionBuilder {
//         total_nodes: 7,
//         start_nodes: 7,
//         num_succeeds: 10,
//         txn_ids: either::Either::Right(1),
//         next_view_timeout: 1000,
//         ids_to_shut_down: vec![vec![6].into_iter().collect::<std::collections::HashSet<_>>()],
//         // overestimate. When VRF election becomes a thing, this is going to need to be infinite
//         failure_threshold: 5,
//         ..hotshot_testing::test_description::GeneralTestDescriptionBuilder::default()
//     },
//     keep: true,
//     slow: false

// );

// // This test simulates two permanent failed nodes
// //
// // With n=7, this is the maximum failures that the network can tolerate
// cross_all_types!(
//     double_permanent_failure,
//     hotshot_testing::test_description::GeneralTestDescriptionBuilder {
//         total_nodes: 7,
//         start_nodes: 7,
//         num_succeeds: 10,
//         txn_ids: either::Either::Right(1),
//         next_view_timeout: 1000,
//         ids_to_shut_down: vec![vec![5, 6].into_iter().collect::<std::collections::HashSet<_>>()],
//         failure_threshold: 5,
//         ..hotshot_testing::test_description::GeneralTestDescriptionBuilder::default()
//     },
//     keep: true,
//     slow: false

// );
