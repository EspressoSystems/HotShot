use std::{sync::Arc, time::Duration};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    marker::PhantomData,
};
use crate::test_launcher::{Networks, TestLauncher};

use hotshot_example_types::state_types::TestInstanceState;
use hotshot_example_types::storage_types::TestStorage;
use hotshot::{
    traits::TestableNodeImplementation, types::SystemContextHandle, HotShotInitializer,
    Memberships, SystemContext,
};
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
use anyhow::Result;
use async_broadcast::{Receiver, SendError, Sender};
use async_compatibility_layer::art::async_timeout;
#[cfg(async_executor_impl = "async-std")]
use async_std::{
    sync::RwLock,
    task::{spawn, JoinHandle},
};
use async_trait::async_trait;
#[cfg(async_executor_impl = "async-std")]
use futures::future::join_all;
#[cfg(async_executor_impl = "tokio")]
use futures::future::try_join_all;
use futures::{
    future::{select, select_all, Either},
    Future,
};
use hotshot_task::task::{Task, TaskEvent, TaskState};
#[cfg(async_executor_impl = "tokio")]
use tokio::{
    sync::RwLock,
    task::{spawn, JoinHandle},
};
use tracing::{error, warn};

/// enum describing how the tasks completed
pub enum TestResult {
    /// the test task passed
    Pass,
    /// the test task failed with an error
    Fail(Box<dyn snafu::Error>),
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

#[async_trait]
/// Type for mutable task state that can be used as the state for a `Task`
pub trait TestTaskState: Send {
    /// Type of event sent and received by the task
    type Event: Clone + Send + Sync;

    /// Handles an event from one of multiple receivers.
    async fn handle_event(&mut self, (event, id): (Arc<Self::Event>, usize)) -> Result<()>;

    /// Check the result of the test.
    fn check(&self) -> TestResult;
}

/// A basic task which loops waiting for events to come from `event_receiver`
/// and then handles them using it's state
/// It sends events to other `Task`s through `event_sender`
/// This should be used as the primary building block for long running
/// or medium running tasks (i.e. anything that can't be described as a dependency task)
pub struct TestTask<S: TestTaskState> {
    /// The state of the task.  It is fed events from `event_sender`
    /// and mutates it state ocordingly.  Also it signals the task
    /// if it is complete/should shutdown
    state: S,
    /// Receives events that are broadcast from any task, including itself
    receivers: Vec<Receiver<Arc<S::Event>>>,
    /// Receiver for test events, used for communication between test tasks.
    test_receiver: Receiver<TestEvent>,
}

#[derive(Clone, Debug)]
pub enum TestEvent {
    Shutdown,
}

impl<S: TestTaskState + Send + 'static> TestTask<S> {
    /// Create a new task
    pub fn new(
        state: S,
        receivers: Vec<Receiver<Arc<S::Event>>>,
        test_receiver: Receiver<TestEvent>,
    ) -> Self {
        TestTask {
            state,
            receivers,
            test_receiver,
        }
    }

    /// The state of the task, as a boxed dynamic trait object.
    fn boxed_state(self) -> Box<dyn TestTaskState<Event = S::Event>> {
        Box::new(self.state) as Box<dyn TestTaskState<Event = S::Event>>
    }

    /// Spawn the task loop, consuming self.  Will continue until
    /// the task reaches some shutdown condition
    pub fn run(mut self) -> JoinHandle<Box<dyn TestTaskState<Event = S::Event>>> {
        spawn(async move {
            loop {
                let mut messages = Vec::new();

                for receiver in &mut self.receivers {
                    messages.push(receiver.recv());
                }

                let test_message = self.test_receiver.recv();

                match select(test_message, select_all(messages)).await {
                    Either::Left((Ok(TestEvent::Shutdown), _)) => {
                        break self.boxed_state();
                    }

                    Either::Right(((Ok(input), id, _), _)) => {
                        let _ = S::handle_event(&mut self.state, (input, id))
                            .await
                            .inspect_err(|e| tracing::error!("{e}"));
                    }

                    Either::Left((Err(e), _)) | Either::Right(((Err(e), _, _), _)) => {
                        error!("Receiver error in test task: {e}");
                    }
                }
            }
        })
    }
}



/// a node participating in a test
pub struct Node<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    /// The node's unique identifier
    pub node_id: u64,
    /// The underlying networks belonging to the node
    pub networks: Networks<TYPES, I>,
    /// The handle to the node's internals
    pub handle: SystemContextHandle<TYPES, I>,
}

/// Either the node context or the parameters to construct the context for nodes that start late.
pub type LateNodeContext<TYPES, I> = Either<
    Arc<SystemContext<TYPES, I>>,
    (
        <I as NodeImplementation<TYPES>>::Storage,
        Memberships<TYPES>,
        HotShotConfig<<TYPES as NodeType>::SignatureKey>,
    ),
>;

/// A yet-to-be-started node that participates in tests
pub struct LateStartNode<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    /// The underlying networks belonging to the node
    pub networks: Networks<TYPES, I>,
    /// Either the context to which we will use to launch HotShot for initialized node when it's
    /// time, or the parameters that will be used to initialize the node and launch HotShot.
    pub context: LateNodeContext<TYPES, I>,
}

/// The runner of a test network
/// spin up and down nodes, execute rounds
pub struct TestRunner<
    TYPES: NodeType,
    I: TestableNodeImplementation<TYPES>,
    N: ConnectedNetwork<Message<TYPES>, TYPES::SignatureKey>,
> {
    /// test launcher, contains a bunch of useful metadata and closures
    pub(crate) launcher: TestLauncher<TYPES, I>,
    /// nodes in the test
    pub(crate) nodes: Vec<Node<TYPES, I>>,
    /// nodes with a late start
    pub(crate) late_start: HashMap<u64, LateStartNode<TYPES, I>>,
    /// the next node unique identifier
    pub(crate) next_node_id: u64,
    /// Phantom for N
    pub(crate) _pd: PhantomData<N>,
}

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
    /// add a specific node with a config
    /// # Panics
    /// if unable to initialize the node's `SystemContext` based on the config
    pub async fn add_node_with_config(
        node_id: u64,
        networks: Networks<TYPES, I>,
        memberships: Memberships<TYPES>,
        initializer: HotShotInitializer<TYPES>,
        config: HotShotConfig<TYPES::SignatureKey>,
        validator_config: ValidatorConfig<TYPES::SignatureKey>,
        storage: I::Storage,
    ) -> Arc<SystemContext<TYPES, I>> {
        // Get key pair for certificate aggregation
        let private_key = validator_config.private_key.clone();
        let public_key = validator_config.public_key.clone();

        let network_bundle = hotshot::Networks {
            quorum_network: networks.0.clone(),
            da_network: networks.1.clone(),
            _pd: PhantomData,
        };

        SystemContext::new(
            public_key,
            private_key,
            node_id,
            config,
            memberships,
            network_bundle,
            initializer,
            ConsensusMetricsValue::default(),
            storage,
        )
        .await
        .expect("Could not init hotshot")
    }
}
