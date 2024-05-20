#![allow(clippy::panic)]
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    marker::PhantomData,
    sync::Arc,
};

use crate::test_task::Node;

use async_lock::RwLock;

use crate::test_task::{LateStartNode, TestRunner, TestEvent, TestResult, TestTask};

use async_broadcast::broadcast;
use futures::future::Either::{self, Left, Right};
use futures::future::join_all;
use hotshot::{
    traits::TestableNodeImplementation, types::SystemContextHandle, HotShotInitializer,
    Memberships, SystemContext,
};
use hotshot_example_types::{state_types::TestInstanceState, storage_types::TestStorage};
use hotshot_types::{
    consensus::ConsensusMetricsValue,
    constants::EVENT_CHANNEL_SIZE,
    data::Leaf,
    message::Message,
    simple_certificate::QuorumCertificate,
    traits::{
        election::Membership,
        network::ConnectedNetwork,
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
    },
    HotShotConfig, ValidatorConfig,
};
#[allow(deprecated)]
use tracing::info;

use super::{
    completion_task::CompletionTask,
    overall_safety_task::{OverallSafetyTask, RoundCtx},
    txn_task::TxnTask,
};
use crate::{
    block_builder::TestBuilderImplementation,
    completion_task::CompletionTaskDescription,
    spinning_task::{ChangeNode, SpinningTask, UpDown},
    test_launcher::{Networks, TestLauncher},
    txn_task::TxnTaskDescription,
    view_sync_task::ViewSyncTask,
};

pub trait TaskErr: std::error::Error + Sync + Send + 'static {}
impl<T: std::error::Error + Sync + Send + 'static> TaskErr for T {}

impl<
        TYPES: NodeType<InstanceState = TestInstanceState>,
        I: TestableNodeImplementation<TYPES>,
        N: ConnectedNetwork<Message<TYPES>, TYPES::SignatureKey>,
    > TestRunner<TYPES, I, N>
where
    I: TestableNodeImplementation<TYPES>,
    I: NodeImplementation<
        TYPES,
        QuorumNetwork = N,
        CommitteeNetwork = N,
        Storage = TestStorage<TYPES>,
    >,
{
    /// execute test
    ///
    /// # Panics
    /// if the test fails
    #[allow(clippy::too_many_lines)]
    pub async fn run_test<B: TestBuilderImplementation<TYPES>>(mut self) {
        let (tx, rx) = broadcast(EVENT_CHANNEL_SIZE);
        let (test_sender, test_receiver) = broadcast(EVENT_CHANNEL_SIZE);
        let spinning_changes = self
            .launcher
            .metadata
            .spinning_properties
            .node_changes
            .clone();

        let mut late_start_nodes: HashSet<u64> = HashSet::new();
        for (_, changes) in &spinning_changes {
            for change in changes {
                if matches!(change.updown, UpDown::Up) {
                    late_start_nodes.insert(change.idx.try_into().unwrap());
                }
            }
        }

        self.add_nodes::<B>(
            self.launcher.metadata.num_nodes_with_stake,
            &late_start_nodes,
        )
        .await;
        let mut event_rxs = vec![];
        let mut internal_event_rxs = vec![];

        for node in &self.nodes {
            let r = node.handle.get_event_stream_known_impl();
            event_rxs.push(r);
        }
        for node in &self.nodes {
            let r = node.handle.get_internal_event_stream_known_impl();
            internal_event_rxs.push(r);
        }

        let TestRunner {
            ref launcher,
            nodes,
            late_start,
            next_node_id: _,
            _pd: _,
        } = self;

        let mut test_generators = vec![];

        let mut task_futs = vec![];
        let meta = launcher.metadata.clone();

        let handles = Arc::new(RwLock::new(nodes));

        let txn_task =
            if let TxnTaskDescription::RoundRobinTimeBased(duration) = meta.txn_description {
                let txn_task = TxnTask {
            handles: Arc::clone(&handles),
                    next_node_idx: Some(0),
                    duration,
                    shutdown_chan: rx.clone(),
                };
                Some(txn_task)
            } else {
                None
            };

        // add completion task
        let CompletionTaskDescription::TimeBasedCompletionTaskBuilder(time_based) =
            meta.completion_task_description;
        let completion_task = CompletionTask {
            tx: tx.clone(),
            rx: rx.clone(),
            handles: Arc::clone(&handles),
            duration: time_based.duration,
        };

        // add spinning task
        // map spinning to view
        let mut changes: BTreeMap<TYPES::Time, Vec<ChangeNode>> = BTreeMap::new();
        for (view, mut change) in spinning_changes {
            changes
                .entry(TYPES::Time::new(view))
                .or_insert_with(Vec::new)
                .append(&mut change);
        }

        let spinning_task_state = SpinningTask {
            handles: Arc::clone(&handles),
            late_start,
            latest_view: None,
            changes,
            last_decided_leaf: Leaf::genesis(&TestInstanceState {}),
            high_qc: QuorumCertificate::genesis(&TestInstanceState {}),
        };
        let spinning_task = TestTask::<SpinningTask<TYPES, I>>::new(
            spinning_task_state,
            event_rxs.clone(),
            test_receiver.clone(),
        );
        // add safety task
        let overall_safety_task_state = OverallSafetyTask {
            handles: Arc::clone(&handles),
            ctx: RoundCtx::default(),
            properties: self.launcher.metadata.overall_safety_properties,
            error: None,
            test_sender, 
        };

        let safety_task = TestTask::<OverallSafetyTask<TYPES, I>>::new(
                overall_safety_task_state,
            event_rxs.clone(),
            test_receiver.clone(),
        );

        // add view sync task
        let view_sync_task_state = ViewSyncTask {
            hit_view_sync: HashSet::new(),
            description: self.launcher.metadata.view_sync_properties,
            _pd: PhantomData,
        };

        let view_sync_task = TestTask::<ViewSyncTask<TYPES, I>>::new(
            view_sync_task_state,
            internal_event_rxs,
            test_receiver.clone(),
        );

        let mut nodes = handles.write().await;

        // wait for networks to be ready
        for node in &mut nodes.iter_mut() {
            node.networks.0.wait_for_ready().await;
            node.networks.1.wait_for_ready().await;
        }

        // Start hotshot
        for node in &mut nodes.iter_mut() {
            if !late_start_nodes.contains(&node.node_id) {
                node.handle.hotshot.start_consensus().await;
            }
        }
        task_futs.push(safety_task.run());
        task_futs.push(view_sync_task.run());
        if let Some(txn) = txn_task {
            test_generators.push(txn.run());
        }
        test_generators.push(completion_task.run());
        task_futs.push(spinning_task.run());
        let mut error_list = vec![];

        #[cfg(async_executor_impl = "async-std")]
        {
            let results = join_all(task_futs).await;
            tracing::info!("test tasks joined");
            for result in results {
                match result {
                    TestResult::Pass => {
                        info!("Task shut down successfully");
                    }
                    TestResult::Fail(e) => error_list.push(e),
                    _ => {
                        panic!("Future impl for task abstraction failed! This should never happen");
                    }
                }
            }
        }

        #[cfg(async_executor_impl = "tokio")]
        {
            let results = join_all(task_futs).await;

            tracing::error!("test tasks joined");
            for result in results {
                match result {
                    Ok(res) => {
                        match res {
                            HotShotTaskCompleted::ShutDown => {
                                info!("Task shut down successfully");
                            }
                            HotShotTaskCompleted::Error(e) => error_list.push(e),
                            _ => {
                                panic!("Future impl for task abstraction failed! This should never happen");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error Joining the test task {:?}", e);
                    }
                }
            }
        }

        assert!(
            error_list.is_empty(),
            "TEST FAILED! Results: {error_list:?}"
        );
    }

    /// Add nodes.
    ///
    /// # Panics
    /// Panics if unable to create a [`HotShotInitializer`]
    pub async fn add_nodes<B: TestBuilderImplementation<TYPES>>(
        &mut self,
        total: usize,
        late_start: &HashSet<u64>,
    ) -> Vec<u64> {
        let mut results = vec![];
        let config = self.launcher.resource_generator.config.clone();
        let known_nodes_with_stake = config.known_nodes_with_stake.clone();

        let (mut builder_task, builder_url) =
            B::start(config.num_nodes_with_stake.into(), B::Config::default()).await;
        for i in 0..total {
            let mut config = config.clone();
            let node_id = self.next_node_id;
            self.next_node_id += 1;
            tracing::debug!("launch node {}", i);

            let memberships = Memberships {
                quorum_membership: <TYPES as NodeType>::Membership::create_election(
                    known_nodes_with_stake.clone(),
                    known_nodes_with_stake.clone(),
                    config.fixed_leader_for_gpuvid,
                ),
                da_membership: <TYPES as NodeType>::Membership::create_election(
                    known_nodes_with_stake.clone(),
                    config.known_da_nodes.clone(),
                    config.fixed_leader_for_gpuvid,
                ),
                vid_membership: <TYPES as NodeType>::Membership::create_election(
                    known_nodes_with_stake.clone(),
                    known_nodes_with_stake.clone(),
                    config.fixed_leader_for_gpuvid,
                ),
                view_sync_membership: <TYPES as NodeType>::Membership::create_election(
                    known_nodes_with_stake.clone(),
                    known_nodes_with_stake.clone(),
                    config.fixed_leader_for_gpuvid,
                ),
            };
            config.builder_url = builder_url.clone();

            let networks = (self.launcher.resource_generator.channel_generator)(node_id).await;
            let storage = (self.launcher.resource_generator.storage)(node_id);

            if self.launcher.metadata.skip_late && late_start.contains(&node_id) {
                self.late_start.insert(
                    node_id,
                    LateStartNode {
                        networks,
                        context: Right((storage, memberships, config)),
                    },
                );
            } else {
                let initializer =
                    HotShotInitializer::<TYPES>::from_genesis(TestInstanceState {}).unwrap();

                // See whether or not we should be DA
                let is_da = node_id < config.da_staked_committee_size as u64;

                // We assign node's public key and stake value rather than read from config file since it's a test
                let validator_config =
                    ValidatorConfig::generated_from_seed_indexed([0u8; 32], node_id, 1, is_da);
                let hotshot = Self::add_node_with_config(
                    node_id,
                    networks.clone(),
                    memberships,
                    initializer,
                    config,
                    validator_config,
                    storage,
                )
                .await;
                if late_start.contains(&node_id) {
                    self.late_start.insert(
                        node_id,
                        LateStartNode {
                            networks,
                            context: Left(hotshot),
                        },
                    );
                } else {
                    let handle = hotshot.run_tasks().await;
                    if node_id == 1 {
                        if let Some(task) = builder_task.take() {
                            task.start(Box::new(handle.get_event_stream()))
                        }
                    }

                    self.nodes.push(Node {
                        node_id,
                        networks,
                        handle,
                    });
                }
            }
            results.push(node_id);
        }

        results
    }
}
