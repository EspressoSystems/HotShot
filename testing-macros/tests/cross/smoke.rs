use hotshot_testing_macros::cross_all_types;

cross_all_types!(
TestName: ten_tx_five_nodes_slow,
TestDescription: hotshot_testing::test_description::GeneralTestDescriptionBuilder {
    total_nodes: 5,
    start_nodes: 5,
    num_succeeds: 10,
    txn_ids: either::Either::Right(1),
    ..hotshot_testing::test_description::GeneralTestDescriptionBuilder::default()
},
Slow: true
);

cross_all_types!(
TestName: ten_tx_seven_nodes_slow,
TestDescription: hotshot_testing::test_description::GeneralTestDescriptionBuilder {
    total_nodes: 7,
    start_nodes: 7,
    num_succeeds: 10,
    txn_ids: either::Either::Right(1),
    ..hotshot_testing::test_description::GeneralTestDescriptionBuilder::default()
},
Slow: true
);
