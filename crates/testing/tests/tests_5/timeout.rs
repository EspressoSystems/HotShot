// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn test_timeout() {
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

    let mut metadata: TestDescription<TestTypes, MemoryImpl, TestVersions> = TestDescription {
        num_nodes_with_stake: 10,
        start_nodes: 10,
        epoch_height: 0,
        ..Default::default()
    };
    let dead_nodes = vec![ChangeNode {
        idx: 0,
        updown: NodeAction::Down,
    }];

    metadata.timing_data = timing_data;

    metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
        num_failed_views: 4,
        num_successful_views: 25,
        ..Default::default()
    };

    metadata.spinning_properties = SpinningTaskDescription {
        node_changes: vec![(5, dead_nodes)],
    };

    metadata.completion_task_description =
        CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(60),
            },
        );

    metadata
        .gen_launcher(0)
        .launch()
        .run_test::<SimpleBuilderImplementation>()
        .await;
}

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_timeout_libp2p() {
    use std::time::Duration;

    use hotshot_example_types::node_types::{Libp2pImpl, TestTypes, TestVersions};
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

    let mut metadata: TestDescription<TestTypes, Libp2pImpl, TestVersions> = TestDescription {
        num_nodes_with_stake: 10,
        start_nodes: 10,
        num_bootstrap_nodes: 10,
        ..Default::default()
    };
    let dead_nodes = vec![ChangeNode {
        idx: 9,
        updown: NodeAction::Down,
    }];

    metadata.timing_data = timing_data;

    metadata.overall_safety_properties = OverallSafetyPropertiesDescription {
        num_failed_views: 25,
        num_successful_views: 25,
        ..Default::default()
    };

    metadata.spinning_properties = SpinningTaskDescription {
        node_changes: vec![(5, dead_nodes)],
    };

    metadata.completion_task_description =
        CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(60),
            },
        );

    metadata
        .gen_launcher(0)
        .launch()
        .run_test::<SimpleBuilderImplementation>()
        .await;
}
