use hotshot_testing_macros::cross_all_types;

// This test simulates a single permanent failed node
cross_all_types!(
    TestName: single_permanent_failure_slow,
    TestBuilder: hotshot_testing::test_builder::TestBuilder {
        metadata: hotshot_testing::test_builder::TestMetadata {
            total_nodes: 7,
            start_nodes: 7,
            num_succeeds: 10,
            timing_data: hotshot_testing::test_builder::TimingData {
                next_view_timeout: 1000,
                ..hotshot_testing::test_builder::TimingData::default()
            },
            failure_threshold: 20,
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
                check: either::Either::Right (hotshot_testing::round_builder::RoundSafetyCheckBuilder {
                    num_failed_rounds_total: 20,
                    ..Default::default()
                }),
                ..Default::default()
            }
        )
    },
    Slow: true,
);

// This test simulates two permanent failed nodes
//
// With n=7, this is the maximum failures that the network can tolerate
cross_all_types!(
    TestName: double_permanent_failure_slow,
    TestBuilder: hotshot_testing::test_builder::TestBuilder {
             metadata: hotshot_testing::test_builder::TestMetadata {
                 total_nodes: 7,
                 start_nodes: 7,
                 num_succeeds: 10,
                 failure_threshold: 20,
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
                     check: either::Either::Right (hotshot_testing::round_builder::RoundSafetyCheckBuilder {
                         num_failed_rounds_total: 20,
                         ..Default::default()
                     }),
                     ..Default::default()
                 }
             )
         }
    Slow: true,
);
