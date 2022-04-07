//! Testing harness for the phaselock repository
//!
//! To build a test environment you can create a [`TestLauncher`] instance. This launcher can be configured to have a custom networking layer, initial state, etc.
//!
//! Calling `TestLauncher::launch()` will turn this launcher into a [`TestRunner`], which can be used to start and stop nodes, send transacstions, etc.
//!
//! Node that `TestLauncher::launch()` is only available if the given `NETWORK`, `STATE` and `STORAGE` are correct.

#![warn(missing_docs)]

mod launcher;

pub use self::launcher::TestLauncher;

use async_std::prelude::FutureExt;
use phaselock::{
    demos::dentry::{
        Account, Addition, Balance, DEntryBlock, State as DemoState, Subtraction, Transaction,
    },
    tc,
    traits::{
        implementations::Stateless, NetworkingImplementation, NodeImplementation, State, Storage,
    },
    types::{EventType, Message, PhaseLockHandle},
    PhaseLock, PhaseLockConfig, H_256,
};
use phaselock_types::traits::BlockContents;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};
use std::{marker::PhantomData, time::Duration};
use tracing::{debug, info, info_span, trace, warn};

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

    /// Run a single round, returning the `STATE` and `Block` of each node in order.
    pub async fn run_one_round(&mut self) -> (Vec<Vec<STATE>>, Vec<Vec<BLOCK>>) {
        let mut blocks = Vec::new();
        let mut states = Vec::new();

        for handle in self.nodes() {
            handle.run_one_round().await;
        }
        let mut failed = false;
        for node in &mut self.nodes {
            let id = node.node_id;
            let mut event = match node
                .handle
                .next_event()
                .timeout(Duration::from_secs(5))
                .await
            {
                Err(_timeout) => {
                    panic!(
                        "PhaseLockHandle {} did not trigger an event within 5 seconds",
                        id
                    );
                }
                Ok(Err(e)) => panic!("PhaseLockHandle {} unexpectedly closed: {:?}", id, e),
                Ok(Ok(event)) => event,
            };
            let mut decide_event = None;

            // drain all events from this node
            loop {
                trace!(?id, ?event);
                match event.event {
                    EventType::ViewTimeout { .. } => {
                        warn!(?event, "Round timed out!");
                        failed = true;
                        break;
                    }
                    x @ EventType::Decide { .. } => decide_event = Some(x),
                    _ => {}
                }

                match node
                    .handle
                    .next_event()
                    .timeout(Duration::from_millis(100))
                    .await
                {
                    Err(_) => {
                        // timeout
                        break;
                    }
                    Ok(Err(e)) => {
                        panic!("Could not get node {}'s event: {:?}", id, e);
                    }
                    Ok(Ok(new_event)) => event = new_event,
                }
            }
            if failed {
                break;
            }
            debug!(?id, "Node reached decision");
            if let Some(EventType::Decide { block, state }) = decide_event {
                blocks.push(block.iter().cloned().collect());
                states.push(state.iter().cloned().collect());
            } else {
                println!("Node {} did not receive a decide event", id);
                println!(
                    "Next event in queue: {:?}",
                    node.handle
                        .next_event()
                        .timeout(Duration::from_secs(1))
                        .await
                );
                println!("(If this is Ok, it means the TestRunner didn't listen for long enough)");
                panic!();
            }
        }
        if failed {
            panic!("Could not run this round, not all nodes reached a decision");
        }
        info!("All nodes reached decision");
        assert_eq!(states.len(), self.nodes.len());
        assert_eq!(blocks.len(), self.nodes.len());

        (states, blocks)
    }

    /// Gracefully shut down this system
    pub async fn shutdown(self) {
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
}

impl<
        NETWORK: NetworkingImplementation<Message<DEntryBlock, Transaction, DemoState, N>> + Clone + 'static,
        STORAGE: Storage<DEntryBlock, DemoState, N> + 'static,
    > TestRunner<NETWORK, STORAGE, DEntryBlock, DemoState>
{
    /// Add a random transaction to this runner.
    ///
    /// Note that this function is only available if `STATE` is [`phaselock::demos::dentry::State`] and `BLOCK` is [`DEntryBlock`].
    pub fn add_random_transaction(&self) -> Result<(), TransactionError> {
        if self.nodes.is_empty() {
            return Err(TransactionError::NoNodes);
        }

        // we're assuming all nodes have the same state
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

        // find a random handle to send this transaction from
        let node = self
            .nodes
            .iter()
            .choose(&mut rng)
            .ok_or(TransactionError::NoNodes)?;
        node.handle
            .submit_transaction_sync(transaction)
            .expect("Could not send transaction");
        Ok(())
    }
}

#[derive(Debug)]
/// Error that is returned from [`TestRunner`] with methods related to transactions
pub enum TransactionError {
    /// There are no valid nodes online
    NoNodes,
    /// There are no valid balances available
    NoValidBalance,
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
