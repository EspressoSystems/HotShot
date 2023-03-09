// use hotshot_testing_macros::cross_tests;

// cross_tests!(
// DemoType: [(ValidatingConsensus, hotshot::demos::vdemo::VDemoState)],
// SignatureKey: [ hotshot_types::traits::signature_key::ed25519::Ed25519Pub ],
// CommChannel: [ hotshot::traits::implementations::MemoryCommChannel ],
// Storage: [ hotshot::traits::implementations::MemoryStorage ],
// Time: [ hotshot_types::data::ViewNumber ],
// TestName: ten_tx_five_nodes_fast,
// TestDescription: hotshot_testing::test_description::GeneralTestDescriptionBuilder {
//     total_nodes: 7,
//     start_nodes: 7,
//     num_succeeds: 10,
//     txn_ids: either::Either::Right(1),
//     ..hotshot_testing::test_description::GeneralTestDescriptionBuilder::default()
// },
// Slow: false,
// );

// cross_tests!(
// DemoType: [(ValidatingConsensus, hotshot::demos::vdemo::VDemoState) ],
// SignatureKey: [ hotshot_types::traits::signature_key::ed25519::Ed25519Pub ],
// CommChannel: [ hotshot::traits::implementations::MemoryCommChannel ],
// Storage: [ hotshot::traits::implementations::MemoryStorage ],
// Time: [ hotshot_types::data::ViewNumber ],
// TestName: ten_tx_seven_nodes_fast,
// TestDescription: hotshot_testing::test_description::GeneralTestDescriptionBuilder {
//     total_nodes: 7,
//     start_nodes: 7,
//     num_succeeds: 10,
//     txn_ids: either::Either::Right(1),
//     ..hotshot_testing::test_description::GeneralTestDescriptionBuilder::default()
// },
// Slow: false,
// );
