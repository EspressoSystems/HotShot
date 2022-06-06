use std::{num::NonZeroUsize, time::Duration};

use super::{Generator, TestRunner, N};
use phaselock::{
    tc::{self},
    traits::{State, Storage},
    types::Message,
    PhaseLockConfig, PubKey,
};
use phaselock_types::traits::{
    network::TestableNetworkingImplementation, state::TestableState, storage::TestableStorage,
    BlockContents,
};
use rand::thread_rng;

/// A launcher for [`TestRunner`], allowing you to customize the network and some default settings for spawning nodes.
pub struct TestLauncher<NETWORK, STORAGE, BLOCK, STATE> {
    pub(super) network: Generator<NETWORK>,
    pub(super) storage: Generator<STORAGE>,
    pub(super) block: Generator<BLOCK>,
    pub(super) state: Generator<STATE>,
    pub(super) config: PhaseLockConfig,
    pub(super) sks: tc::SecretKeySet,
}

impl<
        NETWORK: TestableNetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>>
            + Clone
            + 'static,
        STORAGE: TestableStorage<BLOCK, STATE, N> + 'static,
        BLOCK: BlockContents<N> + Default + 'static,
        STATE: TestableState<N, Block = BLOCK> + 'static,
    > TestLauncher<NETWORK, STORAGE, BLOCK, STATE>
{
    /// Create a new launcher.
    /// Note that `expected_node_count` should be set to an accurate value, as this is used to calculate the `threshold` internally.
    pub fn new(expected_node_count: usize) -> Self {
        let threshold = ((expected_node_count * 2) / 3) + 1;
        let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut thread_rng());

        let known_nodes = (0..expected_node_count)
            .map(|node_id| PubKey::from_secret_key_set_escape_hatch(&sks, node_id as u64))
            .collect();
        let config = PhaseLockConfig {
            total_nodes: NonZeroUsize::new(expected_node_count).unwrap(),
            threshold: NonZeroUsize::new(threshold).unwrap(),
            max_transactions: NonZeroUsize::new(100).unwrap(),
            known_nodes,
            next_view_timeout: 500,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            propose_min_round_time: Duration::from_millis(0),
            propose_max_round_time: Duration::from_millis(1000),
        };

        Self {
            // FIXME pass in number of bootstrap nodes from config
            // instead of just assuming they're 1
            network: NETWORK::generator(expected_node_count, 1, sks.clone()),
            storage: Box::new(|_| {
                <STORAGE as TestableStorage<BLOCK, STATE, N>>::construct_tmp_storage().unwrap()
            }),
            block: Box::new(|_| <BLOCK as Default>::default()),
            state: Box::new(|_| <STATE as TestableState<N>>::get_starting_state()),
            sks,
            config,
        }
    }
}

impl<NETWORK, STORAGE, BLOCK, STATE> TestLauncher<NETWORK, STORAGE, BLOCK, STATE> {
    /// Set a custom network generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_network<NewNetwork>(
        self,
        network: impl Fn(u64, PubKey) -> NewNetwork + 'static,
    ) -> TestLauncher<NewNetwork, STORAGE, BLOCK, STATE> {
        TestLauncher {
            network: Box::new({
                let sks = self.sks.clone();
                move |node_id| {
                    // FIXME perhaps this pk generation is a separate function
                    // to add as an input
                    // that way we don't rely on threshold crypto
                    let pubkey = PubKey::from_secret_key_set_escape_hatch(&sks, node_id);
                    network(node_id, pubkey)
                }
            }),
            storage: self.storage,
            block: self.block,
            state: self.state,
            config: self.config,
            sks: self.sks,
        }
    }

    /// Set a custom storage generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_storage<NewStorage>(
        self,
        storage: impl Fn(u64) -> NewStorage + 'static,
    ) -> TestLauncher<NETWORK, NewStorage, BLOCK, STATE> {
        TestLauncher {
            network: self.network,
            storage: Box::new(storage),
            block: self.block,
            state: self.state,
            config: self.config,
            sks: self.sks,
        }
    }

    /// Set a custom block generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_block<NewBlock>(
        self,
        block: impl Fn(u64) -> NewBlock + 'static,
    ) -> TestLauncher<NETWORK, STORAGE, NewBlock, STATE> {
        TestLauncher {
            network: self.network,
            storage: self.storage,
            block: Box::new(block),
            state: self.state,
            config: self.config,
            sks: self.sks,
        }
    }

    /// Set a custom state generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_state<NewState>(
        self,
        state: impl Fn(u64) -> NewState + 'static,
    ) -> TestLauncher<NETWORK, STORAGE, BLOCK, NewState> {
        TestLauncher {
            network: self.network,
            storage: self.storage,
            block: self.block,
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

    /// Modifies the config used when generating nodes with `f`
    pub fn modify_default_config(mut self, mut f: impl FnMut(&mut PhaseLockConfig)) -> Self {
        f(&mut self.config);
        self
    }
}

impl<
        NETWORK: TestableNetworkingImplementation<Message<BLOCK, BLOCK::Transaction, STATE, N>>
            + Clone
            + 'static,
        STORAGE: Storage<BLOCK, STATE, N>,
        BLOCK: BlockContents<N> + 'static,
        STATE: State<N, Block = BLOCK> + TestableState<N> + 'static,
    > TestLauncher<NETWORK, STORAGE, BLOCK, STATE>
{
    /// Launch the [`TestRunner`]. This function is only available if the following conditions are met:
    ///
    /// - `NETWORK` implements [`NetworkingImplementation`] and [`TestableNetworkingImplementation`]
    /// - `STORAGE` implements [`Storage`]
    /// - `BLOCK` implements [`BlockContents`]
    /// - `STATE` implements [`State`] and [`TestableState`]
    pub fn launch(self) -> TestRunner<NETWORK, STORAGE, BLOCK, STATE> {
        TestRunner::new(self)
    }
}
