use hotshot_testing_macros::cross_tests;

cross_tests!(
     DemoType: [ (ValidatingConsensus, hotshot::demos::vdemo::VDemoState) ],
     SignatureKey: [ hotshot_types::traits::signature_key::ed25519::Ed25519Pub ],
     CommChannel: [ hotshot::traits::implementations::MemoryCommChannel ],
     Storage: [ hotshot::traits::implementations::MemoryStorage ],
     Time: [ hotshot_types::data::ViewNumber ],
     TestName: single_permanent_failure_fast,
     TestBuilder: hotshot_testing::test_builder::TestBuilder {
         metadata: hotshot_testing::test_builder::TestMetadata {
             total_nodes: 7,
             start_nodes: 7,
             num_succeeds: 10,
             timing_data: hotshot_testing::test_builder::TimingData {
                 next_view_timeout: 1000,
                 ..hotshot_testing::test_builder::TimingData::default()
             },
             failure_threshold: 5,
             ..hotshot_testing::test_builder::TestMetadata::default()
         },
         over_ride: Some(
             hotshot_testing::round_builder::RoundBuilder {
                 setup: either::Either::Right(
                     hotshot_testing::round_builder::RoundSetupBuilder {
                         scheduled_changes: vec![
                             hotshot_testing::round_builder::ChangeNode {
                                 idx: 5,
                                 view: 1,
                                 updown: hotshot_testing::round_builder::UpDown::Down
                             },
                         ],
                         ..Default::default()
                     }
                 ),
                 ..Default::default()
             }
         )
     },
    Slow: false,
);

cross_tests!(
     DemoType: [ (ValidatingConsensus, hotshot::demos::vdemo::VDemoState) ],
     SignatureKey: [ hotshot_types::traits::signature_key::ed25519::Ed25519Pub ],
     CommChannel: [ hotshot::traits::implementations::MemoryCommChannel ],
     Storage: [ hotshot::traits::implementations::MemoryStorage ],
     Time: [ hotshot_types::data::ViewNumber ],
     TestName: double_permanent_failure_fast,
     TestBuilder: hotshot_testing::test_builder::TestBuilder {
         metadata: hotshot_testing::test_builder::TestMetadata {
             total_nodes: 7,
             start_nodes: 7,
             num_succeeds: 10,
             timing_data: hotshot_testing::test_builder::TimingData {
                 next_view_timeout: 1000,
                 ..hotshot_testing::test_builder::TimingData::default()
             },
             ..hotshot_testing::test_builder::TestMetadata::default()
         },
         over_ride: Some(
             hotshot_testing::round_builder::RoundBuilder {
                 setup: either::Either::Right(
                     hotshot_testing::round_builder::RoundSetupBuilder {
                         scheduled_changes: vec![
                             hotshot_testing::round_builder::ChangeNode {
                                 idx: 5,
                                 view: 1,
                                 updown: hotshot_testing::round_builder::UpDown::Down
                             },
                             hotshot_testing::round_builder::ChangeNode {
                                 idx: 6,
                                 view: 1,
                                 updown: hotshot_testing::round_builder::UpDown::Down },
                         ],
                         ..Default::default()
                     }
                 ),
                 ..Default::default()
             }
         )
     },
    Slow: false,
);
