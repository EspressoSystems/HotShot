#![allow(clippy::panic)]
use super::{
    completion_task::CompletionTask,
    overall_safety_task::{OverallSafetyTask, RoundCtx},
    txn_task::TxnTask,
};
use crate::{
    completion_task::CompletionTaskDescription,
    spinning_task::{ChangeNode, SpinningTask, UpDown},
    test_launcher::{Networks, TestLauncher},
    txn_task::TxnTaskDescription,
    view_sync_task::ViewSyncTask,
};
use async_broadcast::broadcast;
use futures::future::join_all;
use hotshot::{types::SystemContextHandle, Memberships};

use hotshot::{traits::TestableNodeImplementation, HotShotInitializer, SystemContext};

use hotshot_types::traits::network::CommunicationChannel;
use hotshot_types::{
    consensus::ConsensusMetricsValue,
    traits::{election::Membership, node_implementation::NodeType, state::ConsensusTime},
    HotShotConfig, ValidatorConfig,
};
use std::{
    collections::{HashMap, HashSet},
    marker::PhantomData,
    sync::Arc,
};
use task::task::{Task, TaskRegistry, TestTask};

#[allow(deprecated)]
use tracing::info;

/// a node participating in a test
#[derive(Clone)]
pub struct Node<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    /// the unique identifier of the node
    pub node_id: u64,
    /// the networks of the node
    pub networks: Networks<TYPES, I>,
    /// the handle to the node's internals
    pub handle: SystemContextHandle<TYPES, I>,
}

/// The runner of a test network
/// spin up and down nodes, execute rounds
pub struct TestRunner<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    /// test launcher, contains a bunch of useful metadata and closures
    pub(crate) launcher: TestLauncher<TYPES, I>,
    /// nodes in the test
    pub(crate) nodes: Vec<Node<TYPES, I>>,
    /// nodes with a late start
    pub(crate) late_start: HashMap<u64, SystemContext<TYPES, I>>,
    /// the next node unique identifier
    pub(crate) next_node_id: u64,
}

/// enum describing how the tasks completed
pub enum HotShotTaskCompleted {
    /// the task shut down successfully
    ShutDown,
    /// the task encountered an error
    Error(Box<dyn TaskErr>),
    /// the streams the task was listening for died
    StreamsDied,
    /// we somehow lost the state
    /// this is definitely a bug.
    LostState,
    /// lost the return value somehow
    LostReturnValue,
    /// Stream exists but missing handler
    MissingHandler,
}

pub trait TaskErr: std::error::Error + Sync + Send + 'static {}
impl<T: std::error::Error + Sync + Send + 'static> TaskErr for T {}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> TestRunner<TYPES, I>
where
    I: TestableNodeImplementation<TYPES, CommitteeElectionConfig = TYPES::ElectionConfigType>,
{
    /// excecute test
    /// # Panics
    /// if the test fails
    #[allow(clippy::too_many_lines)]
    pub async fn run_test(mut self) {
        let (tx, rx) = broadcast(100024);
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

        self.add_nodes(self.launcher.metadata.total_nodes, &late_start_nodes)
            .await;
        let mut event_rxs = vec![];
        let mut internal_event_rxs = vec![];

        for node in &self.nodes {
            let r = node.handle.get_event_stream_known_impl().await;
            event_rxs.push(r);
        }
        for node in &self.nodes {
            let r = node.handle.get_internal_event_stream_known_impl().await;
            internal_event_rxs.push(r);
        }

        let reg = Arc::new(TaskRegistry::default());

        let TestRunner {
            ref launcher,
            nodes,
            late_start,
            next_node_id: _,
        } = self;

        let mut task_futs = vec![];
        let meta = launcher.metadata.clone();

        let _txn_task =
            if let TxnTaskDescription::RoundRobinTimeBased(duration) = meta.txn_description {
                let txn_task = TxnTask {
                    handles: nodes.clone(),
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
            handles: nodes.clone(),
            duration: time_based.duration,
        };

        // add spinning task
        // map spinning to view
        let mut changes: HashMap<TYPES::Time, Vec<ChangeNode>> = HashMap::new();
        for (view, mut change) in spinning_changes {
            changes
                .entry(TYPES::Time::new(view))
                .or_insert_with(Vec::new)
                .append(&mut change);
        }

        let spinning_task_state = SpinningTask {
            handles: nodes.clone(),
            late_start,
            latest_view: None,
            changes,
        };
        let spinning_task = TestTask::<SpinningTask<TYPES, I>, SpinningTask<TYPES, I>>::new(
            Task::new(tx.clone(), rx.clone(), reg.clone(), spinning_task_state),
            event_rxs.clone(),
        );
        // add safety task
        let overall_safety_task_state = OverallSafetyTask {
            handles: nodes.clone(),
            ctx: RoundCtx::default(),
            properties: self.launcher.metadata.overall_safety_properties,
        };

        let safety_task = TestTask::<OverallSafetyTask<TYPES, I>, OverallSafetyTask<TYPES, I>>::new(
            Task::new(
                tx.clone(),
                rx.clone(),
                reg.clone(),
                overall_safety_task_state,
            ),
            event_rxs.clone(),
        );

        // add view sync task
        let view_sync_task_state = ViewSyncTask {
            handles: nodes.clone(),
            hit_view_sync: HashSet::new(),
            description: self.launcher.metadata.view_sync_properties,
        };

        let view_sync_task = TestTask::<ViewSyncTask<TYPES, I>, ViewSyncTask<TYPES, I>>::new(
            Task::new(tx.clone(), rx.clone(), reg.clone(), view_sync_task_state),
            internal_event_rxs,
        );

        // wait for networks to be ready
        for node in &nodes {
            node.networks.0.wait_for_ready().await;
        }

        // Start hotshot
        for node in nodes {
            if !late_start_nodes.contains(&node.node_id) {
                node.handle.hotshot.start_consensus().await;
            }
        }
        task_futs.push(safety_task.run());
        task_futs.push(view_sync_task.run());
        // if let Some(txn) = txn_task {
        //     task_futs.push(txn.run());
        // }
        task_futs.push(completion_task.run());
        task_futs.push(spinning_task.run());

        let results = join_all(task_futs).await;
        tracing::error!("test tasks joined");
        let mut error_list = vec![];
        for result in results {
            match result.unwrap() {
                HotShotTaskCompleted::ShutDown => {
                    info!("Task shut down successfully");
                }
                HotShotTaskCompleted::Error(e) => error_list.push(e),
                _ => {
                    panic!("Future impl for task abstraction failed! This should never happen");
                }
            }
        }
        assert!(
            error_list.is_empty(),
            "TEST FAILED! Results: {error_list:?}"
        );
    }

    /// add nodes
    /// # Panics
    /// Panics if unable to create a [`HotShotInitializer`]
    pub async fn add_nodes(&mut self, total: usize, late_start: &HashSet<u64>) -> Vec<u64> {
        let mut results = vec![];
        for i in 0..total {
            tracing::debug!("launch node {}", i);
            let node_id = self.next_node_id;
            let storage = (self.launcher.resource_generator.storage)(node_id);
            let config = self.launcher.resource_generator.config.clone();
            let initializer = HotShotInitializer::<TYPES>::from_genesis().unwrap();
            let networks = (self.launcher.resource_generator.channel_generator)(node_id);
            // We assign node's public key and stake value rather than read from config file since it's a test
            let validator_config =
                ValidatorConfig::generated_from_seed_indexed([0u8; 32], node_id, 1);
            let hotshot = self
                .add_node_with_config(
                    networks.clone(),
                    storage,
                    initializer,
                    config,
                    validator_config,
                )
                .await;
            if late_start.contains(&node_id) {
                self.late_start.insert(node_id, hotshot);
            } else {
                self.nodes.push(Node {
                    node_id,
                    networks,
                    handle: hotshot.run_tasks().await,
                });
            }
            results.push(node_id);
        }

        results
    }

    /// add a specific node with a config
    /// # Panics
    /// if unable to initialize the node's `SystemContext` based on the config
    pub async fn add_node_with_config(
        &mut self,
        networks: Networks<TYPES, I>,
        storage: I::Storage,
        initializer: HotShotInitializer<TYPES>,
        config: HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
        validator_config: ValidatorConfig<TYPES::SignatureKey>,
    ) -> SystemContext<TYPES, I> {
        let node_id = self.next_node_id;
        self.next_node_id += 1;
        let known_nodes_with_stake = config.known_nodes_with_stake.clone();
        // Get key pair for certificate aggregation
        let private_key = validator_config.private_key.clone();
        let public_key = validator_config.public_key.clone();
        let quorum_election_config = config.election_config.clone().unwrap_or_else(|| {
            TYPES::Membership::default_election_config(config.total_nodes.get() as u64)
        });
        let committee_election_config = I::committee_election_config_generator();
        let network_bundle = hotshot::Networks {
            quorum_network: networks.0.clone(),
            da_network: networks.1.clone(),
            _pd: PhantomData,
        };

        let memberships = Memberships {
            quorum_membership: <TYPES as NodeType>::Membership::create_election(
                known_nodes_with_stake.clone(),
                quorum_election_config.clone(),
            ),
            da_membership: <TYPES as NodeType>::Membership::create_election(
                known_nodes_with_stake.clone(),
                committee_election_config(config.da_committee_size as u64),
            ),
            vid_membership: <TYPES as NodeType>::Membership::create_election(
                known_nodes_with_stake.clone(),
                quorum_election_config.clone(),
            ),
            view_sync_membership: <TYPES as NodeType>::Membership::create_election(
                known_nodes_with_stake.clone(),
                quorum_election_config,
            ),
        };

        SystemContext::new(
            public_key,
            private_key,
            node_id,
            config,
            storage,
            memberships,
            network_bundle,
            initializer,
            ConsensusMetricsValue::default(),
        )
        .await
        .expect("Could not init hotshot")
    }
}
