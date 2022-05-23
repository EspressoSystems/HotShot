mod common;
use std::sync::Arc;

use common::*;

use either::Either::Right;

use phaselock::PhaseLockConfig;
use phaselock_testing::TestLauncher;
use tracing::instrument;

/// libp2p network test
#[async_std::test]
#[instrument]
async fn libp2p_network() {
    let gen_runner = Arc::new(|desc: &TestDescription<TestLibp2pNetwork, TestStorage>| {
        // modify runner to recognize timing params
        let set_timing_params = |a: &mut PhaseLockConfig| {
            a.next_view_timeout = desc.timing_config.next_view_timeout;
            a.timeout_ratio = desc.timing_config.timeout_ratio;
            a.round_start_delay = desc.timing_config.round_start_delay;
            a.start_delay = desc.timing_config.start_delay;
        };

        let launcher = TestLauncher::new(desc.total_nodes);

        // one bootstrap
        let generator = TestLibp2pNetwork::generator(desc.total_nodes as u64, 3);

        launcher
            .modify_default_config(set_timing_params)
            .with_network(generator)
            .launch()
    });

    let description = TestDescriptionBuilder {
        next_view_timeout: 600,
        round_start_delay: 25,
        timeout_ratio: (1, 1),
        start_delay: 25,
        total_nodes: 10,
        start_nodes: 10,
        num_succeeds: 1,
        txn_ids: Right(1),
        gen_runner: Some(gen_runner),
        ..TestDescriptionBuilder::<TestLibp2pNetwork, _>::default()
    };

    description.build().execute().await.unwrap();
}
