use std::{marker::PhantomData, num::NonZeroUsize, time::Duration};

use super::{Generator, TestRunner};
use hotshot::{
    traits::{Storage, State, NodeImplementation},
    types::{ed25519::Ed25519Pub, Message, SignatureKey},
};
use hotshot_types::{
    traits::{
        network::TestableNetworkingImplementation,
        signature_key::TestableSignatureKey,
        state::{TestableBlock, TestableState},
        storage::TestableStorage, node_implementation::TestableNodeImplementation,
    },
    ExecutionType, HotShotConfig,
};

/// A launcher for [`TestRunner`], allowing you to customize the network and some default settings for spawning nodes.
pub struct TestLauncher<I: TestableNodeImplementation> {
    pub(super) network: Generator<I::Networking>,
    pub(super) storage: Generator<I::Storage>,
    pub(super) block: Generator<<I::StateType as State>::BlockType>,
    pub(super) config: HotShotConfig<I::SignatureKey>,
}

impl<I: TestableNodeImplementation> TestLauncher<I>
{
    /// Create a new launcher.
    /// Note that `expected_node_count` should be set to an accurate value, as this is used to calculate the `threshold` internally.
    pub fn new(expected_node_count: usize, num_bootstrap_nodes: usize, min_transactions: usize) -> Self {
        let threshold = ((expected_node_count * 2) / 3) + 1;

        let known_nodes = (0..expected_node_count)
            .map(|id| {
                let priv_key = Ed25519Pub::generate_test_key(id as u64);
                Ed25519Pub::from_private(&priv_key)
            })
            .collect();
        let config = HotShotConfig {
            execution_type: ExecutionType::Incremental,
            total_nodes: NonZeroUsize::new(expected_node_count).unwrap(),
            num_bootstrap: num_bootstrap_nodes,
            threshold: NonZeroUsize::new(threshold).unwrap(),
            min_transactions,
            max_transactions: NonZeroUsize::new(99999).unwrap(),
            known_nodes,
            next_view_timeout: 500,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            propose_min_round_time: Duration::from_millis(0),
            propose_max_round_time: Duration::from_millis(1000),
        };

        Self {
            network: I::Networking::generator(expected_node_count, num_bootstrap_nodes),
            storage: Box::new(|_| {
                <I::Storage as TestableStorage<I::StateType>>::construct_tmp_storage().unwrap()
            }),
            block: Box::new(|_| <<I as NodeImplementation>::StateType as State>::BlockType::genesis()),
            config,
        }
    }
}

impl<I: TestableNodeImplementation> TestLauncher<I> {
    /// Set a custom network generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_network<NewNetwork>(
        self,
        network: impl Fn(u64, Ed25519Pub) -> NewNetwork + 'static,
    ) -> TestLauncher<I> {
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
            config: self.config,
        }
    }

    /// Set a custom storage generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_storage<NewStorage>(
        self,
        storage: impl Fn(u64) -> NewStorage + 'static,
    ) -> TestLauncher<I> {
        TestLauncher {
            network: self.network,
            storage: Box::new(storage),
            block: self.block,
            config: self.config,
        }
    }

    /// Set a custom block generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_block<NewBlock>(
        self,
        block: impl Fn(u64) -> NewBlock + 'static,
    ) -> TestLauncher<I> {
        TestLauncher {
            network: self.network,
            storage: self.storage,
            block: Box::new(block),
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

impl<NETWORK, STORAGE, BLOCK, STATE> TestLauncher<NETWORK, STORAGE, BLOCK, STATE>
where
    NETWORK:
        TestableNetworkingImplementation<Message<STATE, Ed25519Pub>, Ed25519Pub> + Clone + 'static,
    STORAGE: Storage<STATE>,
    BLOCK: TestableBlock,
    STATE: TestableState<BlockType = BLOCK> + 'static,
{
    /// Launch the [`TestRunner`]. This function is only available if the following conditions are met:
    ///
    /// - `NETWORK` implements [`NetworkingImplementation`] and [`TestableNetworkingImplementation`]
    /// - `STORAGE` implements [`Storage`]
    /// - `BLOCK` implements [`BlockContents`] and [`TestableBlock`]
    pub fn launch(self) -> TestRunner<NETWORK, STORAGE, STATE> {
        TestRunner::new(self)
    }
}
