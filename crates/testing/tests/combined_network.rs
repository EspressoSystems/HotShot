use std::time::Duration;

use hotshot_testing::{
    completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
    node_types::{CombinedImpl, TestTypes},
    overall_safety_task::OverallSafetyPropertiesDescription,
    spinning_task::{ChangeNode, SpinningTaskDescription, UpDown},
    test_builder::{TestMetadata, TimingData},
};
use rand::Rng;
use tracing::instrument;

use hotshot::traits::implementations::{calculate_hash_of, Cache};
use hotshot_testing::block_types::TestTransaction;

#[cfg(test)]
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[instrument]
async fn test_hash_calculation() {
    let message1 = TestTransaction(vec![0; 32]);
    let message2 = TestTransaction(vec![1; 32]);

    assert_eq!(calculate_hash_of(&message1), calculate_hash_of(&message1));
    assert_ne!(calculate_hash_of(&message1), calculate_hash_of(&message2));
}

#[cfg(test)]
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[instrument]
async fn test_cache_integrity() {
    let message1 = TestTransaction(vec![0; 32]);
    let message2 = TestTransaction(vec![1; 32]);

    let mut cache = Cache::new(3);

    // test insertion integrity
    cache.insert(calculate_hash_of(&message1));
    cache.insert(calculate_hash_of(&message2));

    assert!(cache.contains(calculate_hash_of(&message1)));
    assert!(cache.contains(calculate_hash_of(&message2)));

    // check that the cache is not modified on duplicate entries
    cache.insert(calculate_hash_of(&message1));
    assert!(cache.contains(calculate_hash_of(&message1)));
    assert!(cache.contains(calculate_hash_of(&message2)));
    assert_eq!(cache.len(), 2);
}

/// A run with both the webserver and libp2p functioning properly
#[cfg(test)]
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[instrument]
async fn test_combined_network() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let metadata: TestMetadata = TestMetadata {
        timing_data: TimingData {
            round_start_delay: 25,
            next_view_timeout: 10_000,
            start_delay: 120_000,

            ..Default::default()
        },
        overall_safety_properties: OverallSafetyPropertiesDescription {
            num_failed_views: 33,
            num_successful_views: 25,
            ..Default::default()
        },
        completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(120),
            },
        ),
        ..TestMetadata::default_multiple_rounds()
    };

    metadata
        .gen_launcher::<TestTypes, CombinedImpl>(0)
        .launch()
        .run_test()
        .await;
}

// A run where the webserver crashes part-way through
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[instrument]
async fn test_combined_network_webserver_crash() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let mut metadata: TestMetadata = TestMetadata {
        timing_data: TimingData {
            round_start_delay: 25,
            next_view_timeout: 10_000,
            start_delay: 120_000,

            ..Default::default()
        },
        overall_safety_properties: OverallSafetyPropertiesDescription {
            num_failed_views: 33,
            num_successful_views: 35,
            ..Default::default()
        },
        // allow more time to pass in CI
        completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(120),
            },
        ),
        ..TestMetadata::default_multiple_rounds()
    };

    let mut all_nodes = vec![];
    for node in 0..metadata.total_nodes {
        all_nodes.push(ChangeNode {
            idx: node,
            updown: UpDown::NetworkDown,
        });
    }

    metadata.spinning_properties = SpinningTaskDescription {
        node_changes: vec![(5, all_nodes)],
    };

    metadata
        .gen_launcher::<TestTypes, CombinedImpl>(0)
        .launch()
        .run_test()
        .await;
}

// A run where the webserver crashes partway through
// and then comes back up
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[instrument]
async fn test_combined_network_reup() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let mut metadata: TestMetadata = TestMetadata {
        timing_data: TimingData {
            round_start_delay: 25,
            next_view_timeout: 10_000,
            start_delay: 120_000,

            ..Default::default()
        },
        overall_safety_properties: OverallSafetyPropertiesDescription {
            num_failed_views: 33,
            num_successful_views: 35,
            ..Default::default()
        },
        // allow more time to pass in CI
        completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(120),
            },
        ),
        ..TestMetadata::default_multiple_rounds()
    };

    let mut all_down = vec![];
    let mut all_up = vec![];
    for node in 0..metadata.total_nodes {
        all_down.push(ChangeNode {
            idx: node,
            updown: UpDown::NetworkDown,
        });
        all_up.push(ChangeNode {
            idx: node,
            updown: UpDown::NetworkUp,
        });
    }

    metadata.spinning_properties = SpinningTaskDescription {
        node_changes: vec![(13, all_up), (5, all_down)],
    };

    metadata
        .gen_launcher::<TestTypes, CombinedImpl>(0)
        .launch()
        .run_test()
        .await;
}

// A run where half of the nodes disconnect from the webserver
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[instrument]
async fn test_combined_network_half_dc() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let mut metadata: TestMetadata = TestMetadata {
        timing_data: TimingData {
            round_start_delay: 25,
            next_view_timeout: 10_000,
            start_delay: 120_000,

            ..Default::default()
        },
        overall_safety_properties: OverallSafetyPropertiesDescription {
            num_failed_views: 33,
            num_successful_views: 35,
            ..Default::default()
        },
        // allow more time to pass in CI
        completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(120),
            },
        ),
        ..TestMetadata::default_multiple_rounds()
    };

    let mut half = vec![];
    for node in 0..metadata.total_nodes / 2 {
        half.push(ChangeNode {
            idx: node,
            updown: UpDown::NetworkDown,
        });
    }

    metadata.spinning_properties = SpinningTaskDescription {
        node_changes: vec![(5, half)],
    };

    metadata
        .gen_launcher::<TestTypes, CombinedImpl>(0)
        .launch()
        .run_test()
        .await;
}

fn generate_random_node_changes(
    total_nodes: usize,
    total_num_rounds: usize,
) -> Vec<(u64, Vec<ChangeNode>)> {
    let mut rng = rand::thread_rng();
    let mut node_changes = vec![];

    for _ in 0..total_nodes * 2 {
        let updown = if rng.gen::<bool>() {
            UpDown::NetworkUp
        } else {
            UpDown::NetworkDown
        };

        let node_change = ChangeNode {
            idx: rng.gen_range(0..total_nodes),
            updown,
        };

        let round = rng.gen_range(1..total_num_rounds) as u64;

        node_changes.push((round, vec![node_change]));
    }

    node_changes
}

// A fuzz test, where random network events take place on all nodes
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[instrument]
#[ignore]
async fn test_stress_combined_network_fuzzy() {
    async_compatibility_layer::logging::setup_logging();
    async_compatibility_layer::logging::setup_backtrace();
    let mut metadata = TestMetadata {
        num_bootstrap_nodes: 10,
        total_nodes: 20,
        start_nodes: 20,

        timing_data: TimingData {
            round_start_delay: 25,
            next_view_timeout: 10_000,
            start_delay: 120_000,

            ..Default::default()
        },

        completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
            TimeBasedCompletionTaskDescription {
                duration: Duration::from_secs(120),
            },
        ),
        ..TestMetadata::default_stress()
    };

    metadata.spinning_properties = SpinningTaskDescription {
        node_changes: generate_random_node_changes(
            metadata.total_nodes,
            metadata.overall_safety_properties.num_successful_views * 2,
        ),
    };

    metadata
        .gen_launcher::<TestTypes, CombinedImpl>(0)
        .launch()
        .run_test()
        .await;
}
