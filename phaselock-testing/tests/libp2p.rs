mod common;
use std::{sync::Arc, time::Duration};

use common::*;

use either::Either::Right;

use phaselock::{
    demos::dentry::{DEntryBlock, State as DemoState, Transaction},
    traits::implementations::{Libp2pNetwork, MasterMap, MemoryNetwork},
    types::Message, PhaseLockConfig,
};
use phaselock_testing::TestLauncher;
use tracing::{instrument, error};

/// libp2p network test
#[ignored]
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

        let runner = launcher
            .modify_default_config(set_timing_params)
            .with_network(generator)
            .launch();
        runner
    });

    let description = TestDescriptionBuilder {
        next_view_timeout: 6000,
        round_start_delay: 25,
        timeout_ratio: (1,1),
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

/// normal memory network test
/// here for comparison/debugging only
#[ignored]
#[async_std::test]
#[instrument]
async fn memory_network() {
    let gen_runner = Arc::new(|desc: &TestDescription<MemoryNetwork<_>, TestStorage>| {
        // modify runner to recognize timing params
        let set_timing_params = |a: &mut PhaseLockConfig| {
            a.next_view_timeout = desc.timing_config.next_view_timeout;
            a.timeout_ratio = desc.timing_config.timeout_ratio;
            a.round_start_delay = desc.timing_config.round_start_delay;
            a.start_delay = desc.timing_config.start_delay;
        };

        let launcher = TestLauncher::new(desc.total_nodes);


        let launcher = launcher
            .modify_default_config(set_timing_params)
        .with_network({
            let master_map = MasterMap::new();
            let tmp = desc.network_reliability.clone();
            move |_node_id, pubkey| {
                MemoryNetwork::new(pubkey, master_map.clone(), tmp.clone())
            }
        })
            .launch();
        launcher
    });

    let description = TestDescriptionBuilder {
        next_view_timeout: 60000,
        round_start_delay: 25,
        timeout_ratio: (1,1),
        start_delay: 25,
        total_nodes: 5,
        start_nodes: 5,
        num_succeeds: 1,
        txn_ids: Right(1),
        gen_runner: Some(gen_runner),
        ..TestDescriptionBuilder::<MemoryNetwork<_>, _>::default()
    };

    description.build().execute().await.unwrap();
}
