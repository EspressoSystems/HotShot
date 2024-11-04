// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::time::Duration;

use hotshot_example_types::node_types::{
    CombinedImpl, PushCdnImpl, TestTypes, TestTypesRandomizedLeader, TestVersions,
};
use hotshot_macros::cross_tests;
use hotshot_testing::{
    block_builder::SimpleBuilderImplementation,
    completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
    overall_safety_task::OverallSafetyPropertiesDescription,
    spinning_task::{ChangeNode, NodeAction, SpinningTaskDescription},
    test_builder::{TestDescription, TimingData},
};

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn test_catchup() {
    use std::time::Duration;

    use hotshot_example_types::node_types::{MemoryImpl, TestTypes, TestVersions};
    use hotshot_testing::{
        block_builder::SimpleBuilderImplementation,
        completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
        overall_safety_task::OverallSafetyPropertiesDescription,
        spinning_task::{ChangeNode, NodeAction, SpinningTaskDescription},
        test_builder::{TestDescription, TimingData},
    };

    hotshot::helpers::initialize_logging();

    let timing_data = TimingData {
        next_view_timeout: 2000,
        ..Default::default()
    };
    let mut metadata: TestDescription<TestTypes, MemoryImpl, TestVersions> =
        TestDescription::default();
    let catchup_node = vec![ChangeNode {
        idx: 19,
        updown: NodeAction::Up,
    }];

    metadata.timing_data = timing_data;
    metadata.start_nodes = 19;
    metadata.num_nodes_with_stake = 20;

    metadata.view_sync_properties =
        hotshot_testing::view_sync_task::ViewSyncTaskDescription::Threshold(0, 20);

    metadata.spinning_properties = SpinningTaskDescription {
        // Start the nodes before their leadership.
        node_changes: vec![(13, catchup_node)],
    };

    metadata.completion_task_description =
        CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(60),
            },
        );
    metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
        // Make sure we keep committing rounds after the catchup, but not the full 50.
        num_successful_views: 22,
        num_failed_views: 0,
        ..Default::default()
    };

    metadata
        .gen_launcher(0)
        .launch()
        .run_test::<SimpleBuilderImplementation>()
        .await;
}

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn test_catchup_cdn() {
    use std::time::Duration;

    use hotshot_example_types::node_types::{PushCdnImpl, TestTypes, TestVersions};
    use hotshot_testing::{
        block_builder::SimpleBuilderImplementation,
        completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
        overall_safety_task::OverallSafetyPropertiesDescription,
        spinning_task::{ChangeNode, NodeAction, SpinningTaskDescription},
        test_builder::{TestDescription, TimingData},
    };

    hotshot::helpers::initialize_logging();

    let timing_data = TimingData {
        next_view_timeout: 2000,
        ..Default::default()
    };
    let mut metadata: TestDescription<TestTypes, PushCdnImpl, TestVersions> =
        TestDescription::default();
    let catchup_nodes = vec![ChangeNode {
        idx: 18,
        updown: NodeAction::Up,
    }];
    metadata.timing_data = timing_data;
    metadata.start_nodes = 19;
    metadata.num_nodes_with_stake = 20;

    metadata.spinning_properties = SpinningTaskDescription {
        // Start the nodes before their leadership.
        node_changes: vec![(10, catchup_nodes)],
    };

    metadata.completion_task_description =
        CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_millis(100_000),
            },
        );
    metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
        num_failed_views: 0,
        ..Default::default()
    };

    metadata
        .gen_launcher(0)
        .launch()
        .run_test::<SimpleBuilderImplementation>()
        .await;
}

/// Test that one node catches up and has successful views after coming back
#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn test_catchup_one_node() {
    use std::time::Duration;

    use hotshot_example_types::node_types::{MemoryImpl, TestTypes, TestVersions};
    use hotshot_testing::{
        block_builder::SimpleBuilderImplementation,
        completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
        overall_safety_task::OverallSafetyPropertiesDescription,
        spinning_task::{ChangeNode, NodeAction, SpinningTaskDescription},
        test_builder::{TestDescription, TimingData},
    };
    hotshot::helpers::initialize_logging();

    let timing_data = TimingData {
        next_view_timeout: 2000,
        ..Default::default()
    };
    let mut metadata: TestDescription<TestTypes, MemoryImpl, TestVersions> =
        TestDescription::default();
    let catchup_nodes = vec![ChangeNode {
        idx: 18,
        updown: NodeAction::Up,
    }];
    metadata.timing_data = timing_data;
    metadata.start_nodes = 19;
    metadata.num_nodes_with_stake = 20;

    metadata.spinning_properties = SpinningTaskDescription {
        // Start the nodes before their leadership.
        node_changes: vec![(10, catchup_nodes)],
    };

    metadata.completion_task_description =
        CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(60),
            },
        );
    metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
        // Make sure we keep committing rounds after the catchup, but not the full 50.
        num_successful_views: 22,
        num_failed_views: 0,
        ..Default::default()
    };

    metadata
        .gen_launcher(0)
        .launch()
        .run_test::<SimpleBuilderImplementation>()
        .await;
}

/// Same as `test_catchup` except we start the nodes after their leadership so they join during view sync
#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn test_catchup_in_view_sync() {
    use std::time::Duration;

    use hotshot_example_types::node_types::{MemoryImpl, TestTypes, TestVersions};
    use hotshot_testing::{
        block_builder::SimpleBuilderImplementation,
        completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
        overall_safety_task::OverallSafetyPropertiesDescription,
        spinning_task::{ChangeNode, NodeAction, SpinningTaskDescription},
        test_builder::{TestDescription, TimingData},
    };
    hotshot::helpers::initialize_logging();

    let timing_data = TimingData {
        next_view_timeout: 2000,
        ..Default::default()
    };
    let mut metadata: TestDescription<TestTypes, MemoryImpl, TestVersions> =
        TestDescription::default();
    let catchup_nodes = vec![
        ChangeNode {
            idx: 18,
            updown: NodeAction::Up,
        },
        ChangeNode {
            idx: 19,
            updown: NodeAction::Up,
        },
    ];

    metadata.timing_data = timing_data;
    metadata.start_nodes = 18;
    metadata.num_nodes_with_stake = 20;
    metadata.view_sync_properties =
        hotshot_testing::view_sync_task::ViewSyncTaskDescription::Threshold(0, 20);

    metadata.spinning_properties = SpinningTaskDescription {
        node_changes: vec![(10, catchup_nodes)],
    };

    metadata.completion_task_description =
        CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(60),
            },
        );
    metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
        num_failed_views: 0,
        ..Default::default()
    };

    metadata
        .gen_launcher(0)
        .launch()
        .run_test::<SimpleBuilderImplementation>()
        .await;
}

// Almost the same as `test_catchup`, but with catchup nodes reloaded from anchor leaf rather than
// initialized from genesis.
#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn test_catchup_reload() {
    use std::time::Duration;

    use hotshot_example_types::node_types::{MemoryImpl, TestTypes, TestVersions};
    use hotshot_testing::{
        block_builder::SimpleBuilderImplementation,
        completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
        overall_safety_task::OverallSafetyPropertiesDescription,
        spinning_task::{ChangeNode, NodeAction, SpinningTaskDescription},
        test_builder::{TestDescription, TimingData},
    };

    hotshot::helpers::initialize_logging();

    let timing_data = TimingData {
        next_view_timeout: 2000,
        ..Default::default()
    };
    let mut metadata: TestDescription<TestTypes, MemoryImpl, TestVersions> =
        TestDescription::default();
    let catchup_node = vec![ChangeNode {
        idx: 19,
        updown: NodeAction::Up,
    }];

    metadata.timing_data = timing_data;
    metadata.start_nodes = 19;
    metadata.skip_late = true;
    metadata.num_nodes_with_stake = 20;

    metadata.view_sync_properties =
        hotshot_testing::view_sync_task::ViewSyncTaskDescription::Threshold(0, 20);

    metadata.spinning_properties = SpinningTaskDescription {
        // Start the nodes before their leadership.
        node_changes: vec![(13, catchup_node)],
    };

    metadata.completion_task_description =
        CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(60),
            },
        );
    metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
        // Make sure we keep committing rounds after the catchup, but not the full 50.
        num_successful_views: 22,
        ..Default::default()
    };

    metadata
        .gen_launcher(0)
        .launch()
        .run_test::<SimpleBuilderImplementation>()
        .await;
}

cross_tests!(
    TestName: test_all_restart,
    Impls: [CombinedImpl, PushCdnImpl],
    Types: [TestTypes, TestTypesRandomizedLeader],
    Versions: [TestVersions],
    Ignore: false,
    Metadata: {
      let timing_data = TimingData {
          next_view_timeout: 2000,
          ..Default::default()
      };
      let mut metadata = TestDescription::default();
      let mut catchup_nodes = vec![];

      for i in 0..20 {
          catchup_nodes.push(ChangeNode {
              idx: i,
              updown: NodeAction::RestartDown(0),
          })
      }

      metadata.timing_data = timing_data;
      metadata.start_nodes = 20;
      metadata.num_nodes_with_stake = 20;

      metadata.spinning_properties = SpinningTaskDescription {
          // Restart all the nodes in view 13
          node_changes: vec![(13, catchup_nodes)],
      };
      metadata.view_sync_properties =
          hotshot_testing::view_sync_task::ViewSyncTaskDescription::Threshold(0, 20);

      metadata.completion_task_description =
          CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
              TimeBasedCompletionTaskDescription {
                  duration: Duration::from_secs(60),
              },
          );
      metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
          // Make sure we keep committing rounds after the catchup, but not the full 50.
          num_successful_views: 22,
          num_failed_views: 15,
          ..Default::default()
      };

      metadata
    },
);

// This test case ensures that proposals persist off of a restart. We demonstrate this by
// artificially removing node 0 (the only DA committee member) from the candidate pool,
// meaning that the entire DA also does not have the proposal, but we're still able to
// move on because the *leader* does have the proposal.
cross_tests!(
    TestName: test_all_restart_one_da,
    Impls: [CombinedImpl],
    Types: [TestTypes],
    Versions: [TestVersions],
    Ignore: false,
    Metadata: {
      let timing_data = TimingData {
          next_view_timeout: 2000,
          ..Default::default()
      };
      let mut metadata: TestDescription<TestTypes, CombinedImpl, TestVersions> =
          TestDescription::default();

      let mut catchup_nodes = vec![];
      for i in 0..20 {
          catchup_nodes.push(ChangeNode {
              idx: i,
              updown: NodeAction::RestartDown(0),
          })
      }

      metadata.timing_data = timing_data;
      metadata.start_nodes = 20;
      metadata.num_nodes_with_stake = 20;

      // Explicitly make the DA tiny to exaggerate a missing proposal.
      metadata.da_staked_committee_size = 1;

      metadata.spinning_properties = SpinningTaskDescription {
          // Restart all the nodes in view 13
          node_changes: vec![(13, catchup_nodes)],
      };
      metadata.view_sync_properties =
          hotshot_testing::view_sync_task::ViewSyncTaskDescription::Threshold(0, 20);

      metadata.completion_task_description =
          CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
              TimeBasedCompletionTaskDescription {
                  duration: Duration::from_secs(60),
              },
          );
      metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
          // Make sure we keep committing rounds after the catchup, but not the full 50.
          num_successful_views: 22,
          num_failed_views: 15,
          ..Default::default()
      };


      metadata
    },
);

cross_tests!(
    TestName: test_staggered_restart,
    Impls: [CombinedImpl],
    Types: [TestTypes],
    Versions: [TestVersions],
    Ignore: false,
    Metadata: {
      let mut metadata = TestDescription::default();

      let mut down_da_nodes = vec![];
      for i in 1..4 {
          down_da_nodes.push(ChangeNode {
              idx: i,
              updown: NodeAction::RestartDown(20),
          });
      }

      let mut down_regular_nodes = vec![];
      for i in 4..10 {
          down_regular_nodes.push(ChangeNode {
              idx: i,
              updown: NodeAction::RestartDown(0),
          });
      }
      // restart the last da so it gets the new libp2p routing table
      down_regular_nodes.push(ChangeNode {
          idx: 0,
          updown: NodeAction::RestartDown(0),
      });

      metadata.start_nodes = 10;
      metadata.num_nodes_with_stake = 10;

      // Explicitly make the DA small to simulate real network.
      metadata.da_staked_committee_size = 4;

      metadata.spinning_properties = SpinningTaskDescription {
          // Restart all the nodes in view 13
          node_changes: vec![(13, down_da_nodes), (33, down_regular_nodes)],
      };
      metadata.view_sync_properties =
          hotshot_testing::view_sync_task::ViewSyncTaskDescription::Threshold(0, 50);

      // Give the test some extra time because we are purposely timing out views
      metadata.completion_task_description =
          CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
              TimeBasedCompletionTaskDescription {
                  duration: Duration::from_secs(240),
              },
          );
      metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
          // Make sure we keep committing rounds after the catchup, but not the full 50.
          num_successful_views: 22,
          num_failed_views: 30,
          ..Default::default()
      };

      metadata
    },
);
