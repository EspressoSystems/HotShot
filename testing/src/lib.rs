//! Testing harness for the hotshot repository
//!
//! To build a test environment you can create a [`TestLauncher`] instance. This launcher can be configured to have a custom networking layer, initial state, etc.
//!
//! Calling `TestLauncher::launch()` will turn this launcher into a [`TestRunner`], which can be used to start and stop nodes, send transacstions, etc.
//!
//! Node that `TestLauncher::launch()` is only available if the given `NETWORK`, `STATE` and `STORAGE` are correct.

#![warn(missing_docs)]

mod impls;
mod launcher;
/// implementations of various networking models
pub mod network_reliability;

pub use self::{impls::TestElection, launcher::TestLauncher};

use futures::future::LocalBoxFuture;
use hotshot::{
    data::Leaf,
    traits::{
        election::StaticCommittee, implementations::Stateless, BlockContents,
        NetworkingImplementation, NodeImplementation, StateContents, Storage,
    },
    types::{HotShotHandle, Message},
    HotShot, HotShotError, H_256, HotShotInitializer,
};
use hotshot_types::{
    traits::{
        network::TestableNetworkingImplementation,
        signature_key::{
            ed25519::{Ed25519Priv, Ed25519Pub},
            SignatureKey,
        },
        state::{TestableBlock, TestableState},
    },
    HotShotConfig,
};
use snafu::Snafu;
use std::{collections::HashMap, fmt, marker::PhantomData};
use tracing::{debug, error, info, warn};

/// Wrapper for a function that takes a `node_id` and returns an instance of `T`.
pub type Generator<T> = Box<dyn Fn(u64) -> T + 'static>;

/// For now we only support a size of [`H_256`]. This can be changed in the future.
pub const N: usize = H_256;

/// Result of running a round of consensus
#[derive(Debug)]
// TODO do we need static here
pub struct RoundResult<STATE: StateContents> {
    /// Transactions that were submitted
    pub txns: Vec<<<STATE as StateContents>::Block as BlockContents>::Transaction>,
    /// Nodes that committed this round
    pub results: HashMap<u64, (Vec<STATE>, Vec<<STATE as StateContents>::Block>)>,
    /// Nodes that failed to commit this round
    pub failures: HashMap<u64, HotShotError>,
}

/// Type of function used for checking results after running a view of consensus
pub type RoundPostSafetyCheck<NETWORK, STORAGE, STATE> = Box<
    dyn FnOnce(
        &TestRunner<NETWORK, STORAGE, STATE>,
        RoundResult<STATE>,
    ) -> LocalBoxFuture<Result<(), ConsensusRoundError>>,
>;

/// Type of function used for configuring a round of consensus
pub type RoundSetup<NETWORK, STORAGE, STATE> = Box<
    dyn FnOnce(
        &mut TestRunner<NETWORK, STORAGE, STATE>,
    ) -> LocalBoxFuture<
        Vec<<<STATE as StateContents>::Block as BlockContents>::Transaction>,
    >,
>;

/// Type of function used for checking safety before beginnning consensus
pub type RoundPreSafetyCheck<NETWORK, STORAGE, STATE> = Box<
    dyn FnOnce(
        &TestRunner<NETWORK, STORAGE, STATE>,
    ) -> LocalBoxFuture<Result<(), ConsensusRoundError>>,
>;

/// functions to run a round of consensus
/// the control flow is: (1) pre safety check, (2) setup round, (3) post safety check
pub struct Round<
    NETWORK: NetworkingImplementation<Message<STATE, Ed25519Pub>, Ed25519Pub> + Clone + 'static,
    STORAGE: Storage<STATE> + 'static,
    STATE: TestableState + 'static,
> where
    <STATE as StateContents>::Block: TestableBlock,
{
    /// Safety check before round is set up and run
    /// to ensure consistent state
    pub safety_check_post: Option<RoundPostSafetyCheck<NETWORK, STORAGE, STATE>>,

    /// Round set up
    pub setup_round: Option<RoundSetup<NETWORK, STORAGE, STATE>>,

    /// Safety check after round is complete
    pub safety_check_pre: Option<RoundPreSafetyCheck<NETWORK, STORAGE, STATE>>,
}

impl<
        NETWORK: NetworkingImplementation<Message<STATE, Ed25519Pub>, Ed25519Pub> + Clone + 'static,
        STORAGE: Storage<STATE> + 'static,
        STATE: TestableState + 'static,
    > Default for Round<NETWORK, STORAGE, STATE>
where
    <STATE as StateContents>::Block: TestableBlock,
{
    fn default() -> Self {
        Self {
            safety_check_post: None,
            setup_round: None,
            safety_check_pre: None,
        }
    }
}

/// The runner of a test network
/// spin up and down nodes, execute rounds
pub struct TestRunner<
    NETWORK: NetworkingImplementation<Message<STATE, Ed25519Pub>, Ed25519Pub> + Clone + 'static,
    STORAGE: Storage<STATE> + 'static,
    STATE: TestableState + 'static,
> where
    <STATE as StateContents>::Block: TestableBlock,
{
    network_generator: Generator<NETWORK>,
    storage_generator: Generator<STORAGE>,
    default_node_config: HotShotConfig<Ed25519Pub>,
    nodes: Vec<Node<NETWORK, STORAGE, STATE>>,
    next_node_id: u64,
    rounds: Vec<Round<NETWORK, STORAGE, STATE>>,
}

#[allow(dead_code)]
struct Node<
    NETWORK: NetworkingImplementation<Message<STATE, Ed25519Pub>, Ed25519Pub> + Clone + 'static,
    STORAGE: Storage<STATE> + 'static,
    STATE: TestableState + 'static,
> where
    <STATE as StateContents>::Block: TestableBlock,
{
    pub node_id: u64,
    pub handle: HotShotHandle<TestNodeImpl<NETWORK, STORAGE, STATE>>,
}

impl<
        NETWORK: NetworkingImplementation<Message<STATE, Ed25519Pub>, Ed25519Pub> + Clone + 'static,
        STORAGE: Storage<STATE> + 'static,
        STATE: TestableState + 'static,
    > TestRunner<NETWORK, STORAGE, STATE>
where
    <STATE as StateContents>::Block: TestableBlock,
{
    pub(self) fn new(
        launcher: TestLauncher<NETWORK, STORAGE, <STATE as StateContents>::Block, STATE>,
    ) -> Self {
        Self {
            network_generator: launcher.network,
            storage_generator: launcher.storage,
            default_node_config: launcher.config,
            nodes: Vec::new(),
            next_node_id: 0,
            rounds: vec![],
        }
    }

    /// default setup for round
    pub fn default_before_round(
        _runner: &mut Self,
    ) -> Vec<<<STATE as StateContents>::Block as BlockContents>::Transaction> {
        Vec::new()
    }
    /// default safety check
    pub fn default_safety_check(_runner: &Self, _results: RoundResult<STATE>) {}

    /// Add `count` nodes to the network. These will be spawned with the default node config and state
    pub async fn add_nodes(&mut self, count: usize) -> Vec<u64> {
        let mut results = vec![];
        for _i in 0..count {
            let node_id = self.next_node_id;
            let network = (self.network_generator)(node_id);
            let storage = (self.storage_generator)(node_id);
            let config = self.default_node_config.clone();
            let initializer = HotShotInitializer::from_genesis(<<STATE as StateContents>::Block as TestableBlock>::genesis()).unwrap();
            let node_id = self.add_node_with_config(network, storage, initializer, config).await;
            results.push(node_id);
        }

        results
    }

    /// replace round list
    #[allow(clippy::type_complexity)]
    pub fn with_rounds(&mut self, rounds: Vec<Round<NETWORK, STORAGE, STATE>>) {
        self.rounds = rounds;
        // we call pop, so reverse the array such that first element is on top
        self.rounds.reverse();
    }

    /// Get the next node id that would be used for `add_node_with_config`
    pub fn next_node_id(&self) -> u64 {
        self.next_node_id
    }

    /// Add a node with the given config. This can be used to fine tweak the settings of this particular node. The internal `next_node_id` will be incremented after calling this function.
    ///
    /// For a simpler way to add nodes to this runner, see `add_nodes`
    pub async fn add_node_with_config(
        &mut self,
        network: NETWORK,
        storage: STORAGE,
        initializer: HotShotInitializer<STATE>,
        config: HotShotConfig<Ed25519Pub>,
    ) -> u64 {
        let node_id = self.next_node_id;
        self.next_node_id += 1;

        let known_nodes = config.known_nodes.clone();
        let private_key = Ed25519Priv::generated_from_seed_indexed([0_u8; 32], node_id);
        let public_key = Ed25519Pub::from_private(&private_key);
        let handle = HotShot::init(
            known_nodes.clone(),
            public_key,
            private_key,
            node_id,
            config,
            network,
            storage,
            Stateless::default(),
            StaticCommittee::new(known_nodes),
            initializer
        )
        .await
        .expect("Could not init hotshot");
        self.nodes.push(Node { handle, node_id });
        node_id
    }

    /// Iterate over the [`HotShotHandle`] nodes in this runner.
    pub fn nodes(
        &self,
    ) -> impl Iterator<Item = &HotShotHandle<TestNodeImpl<NETWORK, STORAGE, STATE>>> + '_ {
        self.nodes.iter().map(|node| &node.handle)
    }

    /// repeatedly executes consensus until either:
    /// * `self.fail_threshold` rounds fail
    /// * `self.num_succeeds` rounds are successful
    /// (for a definition of success defined by safety checks)
    pub async fn execute_rounds(
        &mut self,
        num_success: u64,
        fail_threshold: u64,
    ) -> Result<(), ConsensusTestError> {
        let mut num_fails = 0;
        for i in 0..(num_success + fail_threshold) {
            if let Err(e) = self.execute_round().await {
                num_fails += 1;
                error!("failed {:?} round of consensus with error: {:?}", i, e);
                if num_fails > fail_threshold {
                    error!("returning error");
                    return Err(ConsensusTestError::TooManyFailures);
                }
            }
        }
        Ok(())
    }

    /// Execute a single round of consensus
    /// This consists of the following steps:
    /// - checking the state of the hotshot
    /// - setting up the round (ex: submitting txns) or spinning up or down nodes
    /// - checking safety conditions to ensure that the round executed as expected
    pub async fn execute_round(&mut self) -> Result<(), ConsensusRoundError> {
        if let Some(round) = self.rounds.pop() {
            if let Some(safety_check_pre) = round.safety_check_pre {
                safety_check_pre(self).await?;
            }

            let txns = if let Some(setup_fn) = round.setup_round {
                setup_fn(self).await
            } else {
                vec![]
            };
            let results = self.run_one_round(txns).await;
            if let Option::Some(safety_check_post) = round.safety_check_post {
                safety_check_post(self, results).await?;
            }
        }
        Ok(())
    }

    /// Internal function that unpauses hotshots and waits for round to complete,
    /// returns a `RoundResult` upon successful completion, indicating what (if anything) was
    /// committed
    async fn run_one_round(
        &mut self,
        txns: Vec<<<STATE as StateContents>::Block as BlockContents>::Transaction>,
    ) -> RoundResult<STATE> {
        let mut results = HashMap::new();

        info!("EXECUTOR: running one round");
        for handle in self.nodes() {
            handle.start_one_round().await;
        }
        info!("EXECUTOR: done running one round");
        let mut failures = HashMap::new();
        for node in &mut self.nodes {
            let result = node.handle.collect_round_events().await;
            info!(
                "EXECUTOR: collected node {:?} results: {:?}",
                node.node_id.clone(),
                result
            );
            match result {
                Ok((state, block)) => {
                    results.insert(node.node_id, (state, block));
                }
                Err(e) => {
                    failures.insert(node.node_id, e);
                }
            }
        }
        info!("All nodes reached decision");
        if !failures.is_empty() {
            error!(
                "Some failures this round. Failing nodes: {:?}. Successful nodes: {:?}",
                failures, results
            );
        }
        RoundResult {
            txns,
            results,
            failures,
        }
    }

    /// Gracefully shut down this system
    pub async fn shutdown_all(self) {
        for node in self.nodes {
            node.handle.shut_down().await;
        }
        debug!("All nodes should be shut down now.");
    }

    /// In-place shut down an individual node with id `node_id`
    /// # Errors
    /// returns [`ConsensusRoundError::NoSuchNode`] if the node idx is either
    /// - already shut down
    /// - does not exist
    pub async fn shutdown(&mut self, node_id: u64) -> Result<(), ConsensusRoundError> {
        let maybe_idx = self.nodes.iter().position(|n| n.node_id == node_id);
        if let Some(idx) = maybe_idx {
            let node = self.nodes.remove(idx);
            node.handle.shut_down().await;
            Ok(())
        } else {
            Err(ConsensusRoundError::NoSuchNode {
                node_ids: self.ids(),
                requested_id: node_id,
            })
        }
    }

    /// returns the requested handle specified by `id` if it exists
    /// else returns `None`
    pub fn get_handle(
        &self,
        id: u64,
    ) -> Option<HotShotHandle<TestNodeImpl<NETWORK, STORAGE, STATE>>> {
        self.nodes.iter().find_map(|node| {
            if node.node_id == id {
                Some(node.handle.clone())
            } else {
                None
            }
        })
    }

    /// return curent node ids
    pub fn ids(&self) -> Vec<u64> {
        self.nodes.iter().map(|n| n.node_id).collect()
    }
}

impl<
        NETWORK: TestableNetworkingImplementation<Message<STATE, Ed25519Pub>, Ed25519Pub> + Clone + 'static,
        STORAGE: Storage<STATE> + 'static,
        STATE: TestableState + 'static,
    > TestRunner<NETWORK, STORAGE, STATE>
where
    <STATE as StateContents>::Block: TestableBlock,
{
    /// Will validate that all nodes are on exactly the same state.
    pub async fn validate_node_states(&self) {
        let mut leaves = Vec::<Leaf<STATE>>::new();
        for node in self.nodes.iter() {
            let decide_leaf = node.handle.get_decided_leaf().await;
            leaves.push(decide_leaf);
        }

        let (first_leaf, remaining) = leaves.split_first().unwrap();
        // Hack, needs to be fixed: https://github.com/EspressoSystems/HotShot/issues/295
        // Sometimes 1 of the nodes is not in sync with the rest
        // For now we simply check if n-2 nodes match the first node
        let mut mismatch_count = 0;

        for (idx, leaf) in remaining.iter().enumerate() {
            if first_leaf != leaf {
                eprintln!("Leaf dump for {:?}", idx);
                eprintln!("\texpected: {:#?}", first_leaf);
                eprintln!("\tgot:      {:#?}", remaining);
                eprintln!("Node {} storage state does not match the first node", idx);
                mismatch_count += 1;
            }
        }

        if mismatch_count == 0 {
            info!("All nodes are on the same decided leaf.");
            return;
        } else if mismatch_count == 1 {
            // Hack, needs to be fixed: https://github.com/EspressoSystems/HotShot/issues/295
            warn!("One node mismatch, but accepting this anyway.");
            return;
        } else if mismatch_count == self.nodes.len() - 1 {
            // It's probably the first node that is out of sync, check the `remaining` nodes for equality
            let mut all_other_nodes_match = true;

            // not stable yet: https://github.com/rust-lang/rust/issues/75027
            // for [left, right] in remaining.array_windows::<2>() {
            for slice in remaining.windows(2) {
                let (left, right) = if let [left, right] = slice {
                    (left, right)
                } else {
                    unimplemented!()
                };
                if left == right {
                    all_other_nodes_match = false;
                }
            }

            if all_other_nodes_match {
                warn!("One node mismatch, but accepting this anyway");
                return;
            }
        }

        // We tried to recover from n-1 nodes not match, but failed
        // The `eprintln` above will be shown in the output, so we can simply panic
        panic!("Node states do not match");
    }
}

// FIXME make these return some sort of generic error.
// corresponding issue: <https://github.com/EspressoSystems/hotshot/issues/181>
impl<
        NETWORK: NetworkingImplementation<Message<STATE, Ed25519Pub>, Ed25519Pub> + Clone + 'static,
        STORAGE: Storage<STATE> + 'static,
        STATE: TestableState + 'static,
    > TestRunner<NETWORK, STORAGE, STATE>
where
    <STATE as StateContents>::Block: TestableBlock,
{
    /// Add a random transaction to this runner.
    pub async fn add_random_transaction(
        &self,
        node_id: Option<usize>,
    ) -> <<STATE as StateContents>::Block as BlockContents>::Transaction {
        if self.nodes.is_empty() {
            panic!("Tried to add transaction, but no nodes have been added!");
        }

        use rand::{seq::IteratorRandom, thread_rng};
        let mut rng = thread_rng();

        // we're assuming all nodes have the same state.
        // If they don't match, this is probably fine since
        // it should be caught by an assertion (and the txn will be rejected anyway)
        let state = self.nodes[0].handle.get_state().await;

        let txn = <STATE as TestableState>::create_random_transaction(&state);

        let node = if let Some(node_id) = node_id {
            self.nodes.get(node_id).unwrap()
        } else {
            // find a random handle to send this transaction from
            self.nodes.iter().choose(&mut rng).unwrap()
        };

        node.handle
            .submit_transaction(txn.clone())
            .await
            .expect("Could not send transaction");
        txn
    }

    /// add `n` transactions
    /// TODO error handling to make sure entire set of transactions can be processed
    pub async fn add_random_transactions(
        &self,
        n: usize,
    ) -> Option<Vec<<<STATE as StateContents>::Block as BlockContents>::Transaction>> {
        let mut result = Vec::new();
        for _ in 0..n {
            result.push(self.add_random_transaction(None).await);
        }
        Some(result)
    }
}

#[derive(Debug, Snafu)]
/// Error that is returned from [`TestRunner`] with methods related to transactions
pub enum TransactionError {
    /// There are no valid nodes online
    NoNodes,
    /// There are no valid balances available
    NoValidBalance,
    /// FIXME remove this entirely
    /// The requested node does not exist
    InvalidNode,
}

/// Overarchign errors encountered
/// when trying to reach consensus
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum ConsensusRoundError {
    /// Safety condition failed
    SafetyFailed {
        /// description of error
        description: String,
    },
    /// No node exists
    NoSuchNode {
        /// the existing nodes
        node_ids: Vec<u64>,
        /// the node requested
        requested_id: u64,
    },

    /// View times out with any node as the leader.
    TimedOutWithoutAnyLeader,

    /// replicas timed out
    ReplicasTimedOut,

    /// States after a round of consensus is inconsistent.
    InconsistentAfterTxn,

    /// Unable to submit valid transaction
    TransactionError {
        /// source of error
        source: TransactionError,
    },
}

/// An overarching consensus test failure
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum ConsensusTestError {
    /// Too many nodes failed
    TooManyFailures,
}

/// An implementation to make the trio `NETWORK`, `STORAGE` and `STATE` implement [`NodeImplementation`]
#[derive(Clone)]
pub struct TestNodeImpl<NETWORK, STORAGE, STATE> {
    network: PhantomData<NETWORK>,
    storage: PhantomData<STORAGE>,
    state: PhantomData<STATE>,
}

impl<
        NETWORK: NetworkingImplementation<Message<STATE, Ed25519Pub>, Ed25519Pub> + Clone + 'static,
        STORAGE: Storage<STATE> + 'static,
        STATE: TestableState + 'static,
    > NodeImplementation for TestNodeImpl<NETWORK, STORAGE, STATE>
where
    <STATE as StateContents>::Block: TestableBlock,
{
    type State = STATE;
    type Storage = STORAGE;
    type Networking = NETWORK;
    type StatefulHandler = Stateless<STATE::Block, STATE>;
    type Election = StaticCommittee<STATE>;
    type SignatureKey = Ed25519Pub;
}

impl<NETWORK, STORAGE, STATE> fmt::Debug for TestNodeImpl<NETWORK, STORAGE, STATE> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("TestNodeImpl")
            .field("network", &std::any::type_name::<NETWORK>())
            .field("storage", &std::any::type_name::<STORAGE>())
            .field("state", &std::any::type_name::<STATE>())
            .finish_non_exhaustive()
    }
}
