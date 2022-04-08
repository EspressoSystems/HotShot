//! Testing harness for the phaselock repository
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

pub use self::impls::TestElection;
pub use self::launcher::TestLauncher;

use async_std::prelude::FutureExt;
use phaselock::{
    demos::dentry::{
        Account, Addition, Balance, DEntryBlock, State as DemoState, Subtraction, Transaction,
    },
    tc,
    traits::{
        election::StaticCommittee, implementations::Stateless, BlockContents,
        NetworkingImplementation, NodeImplementation, State, Storage,
    },
    types::{EventType, Message, PhaseLockHandle},
    PhaseLock, PhaseLockConfig, PhaseLockError, H_256,
};
use phaselock_types::error::TimeoutSnafu;
use snafu::{ResultExt, Snafu};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt,
};
use std::{marker::PhantomData, time::Duration};
use tracing::{debug, error, info, info_span, warn};

/// Wrapper for a function that takes a `node_id` and returns an instance of `T`.
pub type Generator<T> = Box<dyn Fn(u64) -> T + 'static>;

/// For now we only support a size of [`H_256`]. This can be changed in the future.
pub const N: usize = H_256;

/// The runner of a test network
pub struct TestRunner<
    NETWORK: NetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>> + Clone + 'static,
    STORAGE: Storage<BLOCK, STATE, N> + 'static,
    BLOCK: BlockContents<N> + 'static,
    STATE: State<N, Block = BLOCK> + 'static,
> {
    network_generator: Generator<NETWORK>,
    storage_generator: Generator<STORAGE>,
    block_generator: Generator<BLOCK>,
    state_generator: Generator<STATE>,
    default_node_config: PhaseLockConfig,
    sks: tc::SecretKeySet,
    nodes: Vec<Node<NETWORK, STORAGE, BLOCK, STATE>>,
    next_node_id: u64,
}

#[allow(dead_code)]
struct Node<
    NETWORK: NetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>> + Clone + 'static,
    STORAGE: Storage<BLOCK, STATE, N> + 'static,
    BLOCK: BlockContents<N> + 'static,
    STATE: State<N, Block = BLOCK> + 'static,
> {
    pub node_id: u64,
    pub handle: PhaseLockHandle<TestNodeImpl<NETWORK, STORAGE, BLOCK, STATE>, N>,
}

impl<
        NETWORK: NetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>> + Clone + 'static,
        STORAGE: Storage<BLOCK, STATE, N> + 'static,
        BLOCK: BlockContents<N>,
        STATE: State<N, Block = BLOCK> + 'static,
    > TestRunner<NETWORK, STORAGE, BLOCK, STATE>
{
    pub(self) fn new(launcher: TestLauncher<NETWORK, STORAGE, BLOCK, STATE>) -> Self {
        Self {
            network_generator: launcher.network,
            storage_generator: launcher.storage,
            block_generator: launcher.block,
            state_generator: launcher.state,
            default_node_config: launcher.config,
            sks: launcher.sks,
            nodes: Vec::new(),
            next_node_id: 0,
        }
    }

    /// Add `count` nodes to the network. These will be spawned with the default node config and state
    pub async fn add_nodes(&mut self, count: usize) {
        for _ in 0..count {
            let node_id = self.next_node_id;
            let network = (self.network_generator)(node_id);
            let storage = (self.storage_generator)(node_id);
            let block = (self.block_generator)(node_id);
            let state = (self.state_generator)(node_id);
            let config = self.default_node_config.clone();
            self.add_node_with_config(network, storage, block, state, config)
                .await;
        }
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
        block: BLOCK,
        state: STATE,
        config: PhaseLockConfig,
    ) {
        let node_id = self.next_node_id;
        self.next_node_id += 1;
        let known_nodes = config.known_nodes.clone();
        let handle = PhaseLock::init(
            block,
            self.sks.public_keys(),
            self.sks.secret_key_share(node_id),
            node_id,
            config,
            state,
            network,
            storage,
            Stateless::default(),
            StaticCommittee::new(known_nodes),
        )
        .await
        .expect("Could not init phaselock");
        self.nodes.push(Node { handle, node_id });
    }

    /// Iterate over the [`PhaseLockHandle`] nodes in this runner.
    pub fn nodes(
        &self,
    ) -> impl Iterator<Item = &PhaseLockHandle<TestNodeImpl<NETWORK, STORAGE, BLOCK, STATE>, N>> + '_
    {
        self.nodes.iter().map(|node| &node.handle)
    }

    /// iterate through all events on a [`Node`] and determine if the node finished
    /// successfully
    async fn collect_round_events(
        node: &mut Node<NETWORK, STORAGE, BLOCK, STATE>,
    ) -> Result<(Vec<STATE>, Vec<BLOCK>), PhaseLockError> {
        let id = node.node_id;

        // FIXME add error type with semantic meaning
        // delete unwrap
        let cur_view = node.handle.get_round_runner_state().await.unwrap().view;

        // drain all events from this node
        loop {
            let event = node
                .handle
                .next_event()
                // FIXME use hardcoded timeout
                // for first event
                // then switch to `view_timeout`
                // this hardcoded value is not desirable
                .timeout(Duration::from_secs(10))
                .await
                .context(TimeoutSnafu)??;
            error!(?id, ?event);
            match event.event {
                EventType::ViewTimeout { view_number } => {
                    if view_number >= cur_view {
                        error!(?event, "Round timed out!");
                        return Err(PhaseLockError::ViewTimeoutError { view_number });
                    }
                }
                EventType::Decide { block, state } => {
                    return Ok((
                        state.iter().cloned().collect(),
                        block.iter().cloned().collect(),
                    ));
                }
                event => {
                    debug!("Node {} recv-ed event {:?}", id, event);
                }
            }
        }
    }

    /// Run a single round, returning the `STATE` and `Block` of each node in order.
    pub async fn run_one_round(
        &mut self,
    ) -> (
        HashMap<u64, (Vec<STATE>, Vec<BLOCK>)>,
        HashMap<u64, PhaseLockError>,
    ) {
        let mut results = HashMap::new();

        for handle in self.nodes() {
            handle.run_one_round().await;
        }
        let mut failed_set = HashMap::new();
        for node in &mut self.nodes {
            let result = Self::collect_round_events(node).await;
            match result {
                Ok((state, block)) => {
                    results.insert(node.node_id, (state, block));
                }
                Err(e) => {
                    failed_set.insert(node.node_id, e);
                }
            }
        }
        info!("All nodes reached decision");
        if !failed_set.is_empty() {
            error!(
                "Could not run this round, not all nodes reached a decision. Failing nodes: {:?}",
                failed_set
            );
        }
        (results, failed_set)
    }

    /// Gracefully shut down this system
    pub async fn shutdown_all(self) {
        for node in self.nodes {
            node.handle.shut_down().await;
        }
        debug!("All nodes should be shut down now.");
    }

    /// Will validate that all nodes are on exactly the same state.
    pub async fn validate_node_states(&self) {
        let (first, remaining) = self.nodes.split_first().expect("No nodes registered");

        let runner_state = first
            .handle
            .get_round_runner_state()
            .await
            .expect("Could not get the first node's runner state");
        let storage_state = first.handle.storage().get_internal_state().await;
        // TODO: Should we add this?
        // let network_state = first.handle.networking().get_internal_state().await.expect("Could not get the networking system's internal state");

        info!("Validating node state, comparing with:");
        info!(?runner_state);
        info!(?storage_state);

        let mut is_valid = true;

        for (idx, node) in remaining.iter().enumerate() {
            let idx = idx + 1;
            let span = info_span!("Node {}", idx);
            let _guard = span.enter();
            let comparison_runner_state = node
                .handle
                .get_round_runner_state()
                .await
                .expect("Could not get the node's runner state");
            let comparison_storage_state = node.handle.storage().get_internal_state().await;
            if comparison_runner_state != runner_state {
                eprintln!("Node {} runner state does not match the first node", idx);
                eprintln!("  expected: {:#?}", runner_state);
                eprintln!("  got:      {:#?}", comparison_runner_state);
                is_valid = false;
            }
            if comparison_storage_state != storage_state {
                eprintln!("Node {} storage state does not match the first node", idx);
                eprintln!("  expected: {:#?}", storage_state);
                eprintln!("  got:      {:#?}", comparison_storage_state);
                is_valid = false;
            }
        }
        assert!(is_valid, "Nodes had a different state");
        info!("All nodes are on the same state.");
    }

    /// In-place shut down an individual node with id `node_id`
    /// # Errors
    /// returns [`ConsensusTestError::NoSuchNode`] if the node idx is either
    /// - already shut down
    /// - does not exist
    pub async fn shutdown(&mut self, node_id: u64) -> Result<(), ConsensusTestError> {
        let maybe_idx = self.nodes.iter().position(|n| n.node_id == node_id);
        if let Some(idx) = maybe_idx {
            let node = self.nodes.remove(idx);
            node.handle.shut_down().await;
            Ok(())
        } else {
            Err(ConsensusTestError::NoSuchNode {
                node_ids: self.ids(),
                requested_id: node_id,
            })
        }
    }

    /// return curent node ids
    pub fn ids(&mut self) -> Vec<u64> {
        self.nodes.iter().map(|n| n.node_id).collect()
    }
}

impl<
        NETWORK: NetworkingImplementation<Message<DEntryBlock, Transaction, DemoState, N>> + Clone + 'static,
        STORAGE: Storage<DEntryBlock, DemoState, N> + 'static,
    > TestRunner<NETWORK, STORAGE, DEntryBlock, DemoState>
{
    /// Add a random transaction to this runner.
    ///
    /// Note that this function is only available if `STATE` is [`phaselock::demos::dentry::State`].
    pub fn add_random_transaction(
        &self,
        node_id: Option<usize>,
    ) -> Result<Transaction, TransactionError> {
        if self.nodes.is_empty() {
            return Err(TransactionError::NoNodes);
        }

        // we're assuming all nodes have the same state
        // FIXME it may be good to do an assertion on the state matching
        let state = self.nodes[0].handle.get_state_sync();

        use rand::{seq::IteratorRandom, thread_rng, Rng};
        let mut rng = thread_rng();

        // Only get the balances that have an actual value
        let non_zero_balances = state
            .balances
            .iter()
            .filter(|b| *b.1 > 0)
            .collect::<Vec<_>>();
        if non_zero_balances.is_empty() {
            return Err(TransactionError::NoValidBalance);
        }

        let input_account = non_zero_balances
            .iter()
            .choose(&mut rng)
            .ok_or(TransactionError::NoValidBalance)?
            .0;
        let output_account = state
            .balances
            .keys()
            .choose(&mut rng)
            .ok_or(TransactionError::NoValidBalance)?;
        let amount = rng.gen_range(0, state.balances[input_account]);
        let transaction = Transaction {
            add: Addition {
                account: output_account.to_string(),
                amount,
            },
            sub: Subtraction {
                account: input_account.to_string(),
                amount,
            },
            nonce: rng.gen(),
        };

        let node = if let Some(node_id) = node_id {
            self.nodes
                .get(node_id)
                .ok_or(TransactionError::InvalidNode)?
        } else {
            // find a random handle to send this transaction from
            self.nodes
                .iter()
                .choose(&mut rng)
                .ok_or(TransactionError::NoNodes)?
        };

        node.handle
            .submit_transaction_sync(transaction.clone())
            .expect("Could not send transaction");
        Ok(transaction)
    }

    /// add `n` transactions
    /// TODO error handling to make sure entire set of transactions can be processed
    pub fn add_random_transactions(
        &mut self,
        n: usize,
    ) -> Result<Vec<Transaction>, TransactionError> {
        let mut result = Vec::new();
        for _ in 0..n {
            result.push(self.add_random_transaction(None)?);
        }
        Ok(result)
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
pub enum ConsensusTestError {
    /// No node exists
    NoSuchNode {
        /// the existing nodes
        node_ids: Vec<u64>,
        /// the node requested
        requested_id: u64,
    },

    /// View times out with any node as the leader.
    TimedOutWithoutAnyLeader,

    /// States after a round of consensus is inconsistent.
    InconsistentAfterTxn,

    /// Unable to submit valid transaction
    TransactionError {
        /// source of error
        source: TransactionError,
    },
}

/// An implementation to make the trio `NETWORK`, `STORAGE` and `STATE` implement [`NodeImplementation`]
#[derive(Clone)]
pub struct TestNodeImpl<NETWORK, STORAGE, BLOCK, STATE> {
    network: PhantomData<NETWORK>,
    storage: PhantomData<STORAGE>,
    state: PhantomData<STATE>,
    block: PhantomData<BLOCK>,
}

impl<
        NETWORK: NetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>> + Clone + 'static,
        STORAGE: Storage<BLOCK, STATE, N> + 'static,
        BLOCK: BlockContents<N> + 'static,
        STATE: State<N, Block = BLOCK> + 'static,
    > NodeImplementation<N> for TestNodeImpl<NETWORK, STORAGE, BLOCK, STATE>
{
    type Block = BLOCK;
    type State = STATE;
    type Storage = STORAGE;
    type Networking = NETWORK;
    type StatefulHandler = Stateless<BLOCK, STATE, N>;
    type Election = StaticCommittee<STATE, N>;
}

impl<NETWORK, STORAGE, BLOCK, STATE> fmt::Debug for TestNodeImpl<NETWORK, STORAGE, BLOCK, STATE> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("TestNodeImpl")
            .field("network", &std::any::type_name::<NETWORK>())
            .field("storage", &std::any::type_name::<STORAGE>())
            .field("block", &std::any::type_name::<BLOCK>())
            .field("state", &std::any::type_name::<STATE>())
            .finish_non_exhaustive()
    }
}

/// Provides a common starting state
pub fn get_starting_state() -> DemoState {
    let balances: BTreeMap<Account, Balance> = vec![
        ("Joe", 1_000_000),
        ("Nathan M", 500_000),
        ("John", 400_000),
        ("Nathan Y", 600_000),
        ("Ian", 0),
    ]
    .into_iter()
    .map(|(x, y)| (x.to_string(), y))
    .collect();
    DemoState {
        balances,
        nonces: BTreeSet::default(),
    }
}
