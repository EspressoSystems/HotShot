use super::{Block, Generator, TestRunner, N};
use phaselock::{
    demos::dentry::{State as DemoState, Transaction},
    tc,
    traits::{
        implementations::{DummyReliability, MasterMap, MemoryNetwork, MemoryStorage},
        NetworkingImplementation, State, Storage,
    },
    types::Message,
    PhaseLockConfig, PubKey,
};
use rand::thread_rng;

/// A launcher for [`TestRunner`], allowing you to customize the network and some default settings for spawning nodes.
pub struct TestLauncher<NETWORK, STORAGE, STATE> {
    pub(super) network: Generator<NETWORK>,
    pub(super) storage: Generator<STORAGE>,
    pub(super) state: Generator<STATE>,
    pub(super) config: PhaseLockConfig,
    pub(super) sks: tc::SecretKeySet,
}

impl
    TestLauncher<
        MemoryNetwork<Message<Block, Transaction, DemoState, N>>,
        MemoryStorage<Block, DemoState, N>,
        DemoState,
    >
{
    /// Create a new launcher.
    /// Note that `expected_node_count` should be set to an accurate value, as this is used to calculate the `threshold` internally.
    pub fn new(expected_node_count: usize) -> Self {
        let threshold = ((expected_node_count * 2) / 3) + 1;
        let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut thread_rng());
        let master = MasterMap::new();

        let known_nodes: Vec<PubKey> = (0..expected_node_count)
            .map(|node_id| PubKey::from_secret_key_set_escape_hatch(&sks, node_id as u64))
            .collect();
        let config = PhaseLockConfig {
            total_nodes: expected_node_count as u32,
            threshold: threshold as u32,
            max_transactions: 100,
            known_nodes,
            next_view_timeout: 100,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
        };

        Self {
            network: Box::new({
                let sks = sks.clone();
                move |node_id| {
                    let pubkey = PubKey::from_secret_key_set_escape_hatch(&sks, node_id);
                    MemoryNetwork::new(pubkey, master.clone(), Option::<DummyReliability>::None)
                }
            }),
            storage: Box::new(|_| MemoryStorage::default()),
            state: Box::new(|_| super::get_starting_state()),
            sks,
            config,
        }
    }
}

impl<NETWORK, STORAGE, STATE> TestLauncher<NETWORK, STORAGE, STATE> {
    /// Set a custom network generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_network<NewNetwork>(
        self,
        network: impl Fn(u64) -> NewNetwork + 'static,
    ) -> TestLauncher<NewNetwork, STORAGE, STATE> {
        TestLauncher {
            network: Box::new(network),
            storage: self.storage,
            state: self.state,
            config: self.config,
            sks: self.sks,
        }
    }

    /// Set a custom storage generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_storage<NewStorage>(
        self,
        storage: impl Fn(u64) -> NewStorage + 'static,
    ) -> TestLauncher<NETWORK, NewStorage, STATE> {
        TestLauncher {
            network: self.network,
            storage: Box::new(storage),
            state: self.state,
            config: self.config,
            sks: self.sks,
        }
    }

    /// Set a custom state generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_state<NewState>(
        self,
        state: impl Fn(u64) -> NewState + 'static,
    ) -> TestLauncher<NETWORK, STORAGE, NewState> {
        TestLauncher {
            network: self.network,
            storage: self.storage,
            state: Box::new(state),
            config: self.config,
            sks: self.sks,
        }
    }

    /// Set the default config of each node. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_default_config(mut self, config: PhaseLockConfig) -> Self {
        self.config = config;
        self
    }
}

impl<
        NETWORK: NetworkingImplementation<Message<Block, Transaction, STATE, N>> + Clone + 'static,
        STORAGE: Storage<Block, STATE, N>,
        STATE: State<N, Block = Block> + 'static,
    > TestLauncher<NETWORK, STORAGE, STATE>
{
    /// Launch the [`TestRunner`]. This function is only available if the following conditions are met:
    ///
    /// - `NETWORK` implements [`NetworkingImplementation`]
    /// - `STORAGE` implements [`Storage`]
    /// - `STATE` implements [`State`]
    pub fn launch(self) -> TestRunner<NETWORK, STORAGE, STATE> {
        TestRunner::new(self)
    }
}
