use std::{num::NonZeroUsize, time::Duration};

use super::{Generator, TestRunner};
use hotshot::{
    traits::{StateContents, Storage},
    types::Message,
    HotShotConfig,
};
use hotshot_types::traits::{
    block_contents::Genesis,
    network::TestableNetworkingImplementation,
    signature_key::{ed25519::Ed25519Pub, SignatureKey, TestableSignatureKey},
    state::{TestableBlock, TestableState},
    storage::TestableStorage,
    BlockContents,
};

/// A launcher for [`TestRunner`], allowing you to customize the network and some default settings for spawning nodes.
pub struct TestLauncher<NETWORK, STORAGE, BLOCK, STATE> {
    pub(super) network: Generator<NETWORK>,
    pub(super) storage: Generator<STORAGE>,
    pub(super) block: Generator<BLOCK>,
    pub(super) state: Generator<STATE>,
    pub(super) config: HotShotConfig<Ed25519Pub>,
}

impl<
        NETWORK: TestableNetworkingImplementation<Message<STATE, Ed25519Pub>, Ed25519Pub> + Clone + 'static,
        STORAGE: TestableStorage<STATE> + 'static,
        BLOCK: BlockContents + TestableBlock + 'static,
        STATE: TestableState<Block = BLOCK> + 'static,
    > TestLauncher<NETWORK, STORAGE, BLOCK, STATE>
{
    /// Create a new launcher.
    /// Note that `expected_node_count` should be set to an accurate value, as this is used to calculate the `threshold` internally.
    pub fn new(expected_node_count: usize, num_bootstrap_nodes: usize) -> Self {
        let threshold = ((expected_node_count * 2) / 3) + 1;

        let known_nodes = (0..expected_node_count)
            .map(|id| {
                let priv_key = Ed25519Pub::generate_test_key(id as u64);
                Ed25519Pub::from_private(&priv_key)
            })
            .collect();
        let config = HotShotConfig {
            execution_type: hotshot::ExecutionType::Incremental,
            total_nodes: NonZeroUsize::new(expected_node_count).unwrap(),
            num_bootstrap: num_bootstrap_nodes,
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
            network: NETWORK::generator(expected_node_count, num_bootstrap_nodes),
            storage: Box::new(|_| {
                <STORAGE as TestableStorage<STATE>>::construct_tmp_storage(
                    <STATE::Block as Genesis>::genesis(),
                    <STATE as Genesis>::genesis(),
                )
                .unwrap()
            }),
            block: Box::new(|_| <BLOCK as Genesis>::genesis()),
            state: Box::new(|_| <STATE as Genesis>::genesis()),
            config,
        }
    }
}

impl<NETWORK, STORAGE, BLOCK, STATE> TestLauncher<NETWORK, STORAGE, BLOCK, STATE> {
    /// Set a custom network generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_network<NewNetwork>(
        self,
        network: impl Fn(u64, Ed25519Pub) -> NewNetwork + 'static,
    ) -> TestLauncher<NewNetwork, STORAGE, BLOCK, STATE> {
        TestLauncher {
            network: Box::new({
                move |node_id| {
                    // FIXME perhaps this pk generation is a separate function
                    // to add as an input
                    // that way we don't rely on threshold crypto
                    let priv_key = Ed25519Pub::generate_test_key(node_id);
                    let pubkey = Ed25519Pub::from_private(&priv_key);
                    network(node_id, pubkey)
                }
            }),
            storage: self.storage,
            block: self.block,
            state: self.state,
            config: self.config,
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
        }
    }

    /// Set the default config of each node. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_default_config(mut self, config: HotShotConfig<Ed25519Pub>) -> Self {
        self.config = config;
        self
    }

    /// Modifies the config used when generating nodes with `f`
    pub fn modify_default_config(
        mut self,
        mut f: impl FnMut(&mut HotShotConfig<Ed25519Pub>),
    ) -> Self {
        f(&mut self.config);
        self
    }
}

impl<
        NETWORK: TestableNetworkingImplementation<Message<STATE, Ed25519Pub>, Ed25519Pub> + Clone + 'static,
        STORAGE: Storage<STATE>,
        BLOCK: TestableBlock,
        STATE: StateContents<Block = BLOCK> + TestableState + 'static,
    > TestLauncher<NETWORK, STORAGE, BLOCK, STATE>
{
    /// Launch the [`TestRunner`]. This function is only available if the following conditions are met:
    ///
    /// - `NETWORK` implements [`NetworkingImplementation`] and [`TestableNetworkingImplementation`]
    /// - `STORAGE` implements [`Storage`]
    /// - `BLOCK` implements [`BlockContents`]
    /// - `STATE` implements [`State`] and [`TestableState`]
    pub fn launch(self) -> TestRunner<NETWORK, STORAGE, STATE> {
        TestRunner::new(self)
    }
}
