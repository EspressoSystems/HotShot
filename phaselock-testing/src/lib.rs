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

use async_std::task::JoinHandle;
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
use std::marker::PhantomData;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};
use tracing::{debug, info, trace, warn};

/// Wrapper for a function that takes a `node_id` and returns an instance of `T`.
pub type Generator<T> = Box<dyn Fn(u64) -> T + 'static>;

/// For now we only support [`DEntryBlock`] as `Block` type. This can be changed in the future.
pub type Block = DEntryBlock;

/// For now we only support a size of [`H_256`]. This can be changed in the future.
pub const N: usize = H_256;

/// The runner of a test network
pub struct TestRunner<
    NETWORK: NetworkingImplementation<Message<Block, Transaction, STATE, N>> + Clone + 'static,
    STORAGE: Storage<Block, STATE, N> + 'static,
    STATE: State<N, Block = Block> + 'static,
> {
    network_generator: Generator<NETWORK>,
    storage_generator: Generator<STORAGE>,
    state_generator: Generator<STATE>,
    default_node_config: PhaseLockConfig,
    sks: tc::SecretKeySet,
    nodes: Vec<Node<NETWORK, STORAGE, STATE>>,
    next_node_id: u64,
}

#[allow(dead_code)]
struct Node<
    NETWORK: NetworkingImplementation<Message<Block, Transaction, STATE, N>> + Clone + 'static,
    STORAGE: Storage<Block, STATE, N> + 'static,
    STATE: State<N, Block = Block> + 'static,
> {
    pub node_id: u64,
    pub handle: PhaseLockHandle<TestNodeImpl<NETWORK, STORAGE, STATE>, N>,
    pub join_handle: JoinHandle<()>,
}

impl<
        NETWORK: NetworkingImplementation<Message<Block, Transaction, STATE, N>> + Clone + 'static,
        STORAGE: Storage<Block, STATE, N> + 'static,
        STATE: State<N, Block = Block> + 'static,
    > TestRunner<NETWORK, STORAGE, STATE>
{
    pub(self) fn new(launcher: TestLauncher<NETWORK, STORAGE, STATE>) -> Self {
        Self {
            network_generator: launcher.network,
            storage_generator: launcher.storage,
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
            let state = (self.state_generator)(node_id);
            let network = (self.network_generator)(node_id);
            let storage = (self.storage_generator)(node_id);
            self.add_node_with_config(state, network, storage, self.default_node_config.clone())
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
        state: STATE,
        network: NETWORK,
        storage: STORAGE,
        config: PhaseLockConfig,
    ) {
        let node_id = self.next_node_id;
        self.next_node_id += 1;
        let (join_handle, handle) = PhaseLock::init(
            Block::default(),
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
        self.nodes.push(Node {
            join_handle,
            handle,
            node_id,
        });
    }

    /// Iterate over the [`PhaseLockHandle`] nodes in this runner.
    pub fn nodes(
        &self,
    ) -> impl Iterator<Item = &PhaseLockHandle<TestNodeImpl<NETWORK, STORAGE, STATE>, N>> + '_ {
        self.nodes.iter().map(|node| &node.handle)
    }

    /// Run a single round, returning the `STATE` and `Block` of each node in order.
    pub async fn run_one_round(&mut self) -> (Vec<Vec<STATE>>, Vec<Vec<Block>>) {
        let mut blocks = Vec::new();
        let mut states = Vec::new();

        for handle in self.nodes() {
            handle.run_one_round().await;
        }
        let mut failed = false;
        for node in &mut self.nodes {
            let id = node.node_id;
            let mut event = node
                .handle
                .next_event()
                .await
                .expect("PhaseLock unexpectedly closed");
            while !matches!(event.event, EventType::Decide { .. }) {
                if matches!(event.event, EventType::ViewTimeout { .. }) {
                    warn!(?event, "Round timed out!");
                    failed = true;
                    break;
                }
                trace!(?id, ?event);
                event = node
                    .handle
                    .next_event()
                    .await
                    .expect("PhaseLock unexpectedly closed");
            }
            if failed {
                // put the tx back where we found it and break
                break;
            }
            debug!(?id, "Node reached decision");
            if let EventType::Decide { block, state } = event.event {
                blocks.push(block.iter().cloned().collect());
                states.push(state.iter().cloned().collect());
            } else {
                unreachable!()
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
}

impl<
        NETWORK: NetworkingImplementation<Message<Block, Transaction, DemoState, N>> + Clone + 'static,
        STORAGE: Storage<Block, DemoState, N> + 'static,
    > TestRunner<NETWORK, STORAGE, DemoState>
{
    /// Add a random transaction to this runner.
    ///
    /// Note that this function is only available if `STATE` is [`phaselock::demos::dentry::State`].
    pub fn add_random_transaction(&self) {
        // we're assuming all nodes have the same state
        let state = self.nodes[0].handle.get_state_sync();

        use rand::{seq::IteratorRandom, thread_rng, Rng};
        let mut rng = thread_rng();
        let input_account = state.balances.keys().choose(&mut rng).unwrap();
        let output_account = state.balances.keys().choose(&mut rng).unwrap();
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
        let node = self.nodes.iter().choose(&mut rng).unwrap();
        node.handle
            .submit_transaction_sync(transaction)
            .expect("Could not send transaction");
    }
}

/// An implementation to make the trio `NETWORK`, `STORAGE` and `STATE` implement [`NodeImplementation`]
#[derive(Clone)]
pub struct TestNodeImpl<NETWORK, STORAGE, STATE> {
    network: PhantomData<NETWORK>,
    storage: PhantomData<STORAGE>,
    state: PhantomData<STATE>,
}

impl<
        NETWORK: NetworkingImplementation<Message<Block, Transaction, STATE, N>> + Clone + 'static,
        STORAGE: Storage<Block, STATE, N> + 'static,
        STATE: State<N, Block = Block> + 'static,
    > NodeImplementation<N> for TestNodeImpl<NETWORK, STORAGE, STATE>
{
    type Block = Block;
    type State = STATE;
    type Storage = STORAGE;
    type Networking = NETWORK;
    type StatefulHandler = Stateless<Block, STATE, N>;
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
