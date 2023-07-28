use std::time::Duration;

use async_compatibility_layer::logging::{shutdown_logging, setup_logging, setup_backtrace};
use hotshot_testing::{
    node_types::{SequencingTestTypes, SequencingWebImpl},
    test_builder::{TestMetadata, TimingData}, txn_task::TxnTaskDescription, completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
};
use tracing::instrument;

/// Web server network test
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn web_server_network() {
    setup_logging();
    setup_backtrace();
    let metadata = TestMetadata {
        timing_data: TimingData {
            round_start_delay: 1000,
            next_view_timeout: 3000,
            start_delay: 2000,
            ..Default::default()
        },
        completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(TimeBasedCompletionTaskDescription{duration: Duration::new(60, 0)}),
        txn_description: TxnTaskDescription::RoundRobinTimeBased(Duration::new(0, 500)),
        ..TestMetadata::default()
    };
    metadata
        .gen_launcher::<SequencingTestTypes, SequencingWebImpl>()
        .launch()
        .run_test()
        .await;
    shutdown_logging();
}
