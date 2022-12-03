use super::{Generator, TestRunner};
use hotshot::types::SignatureKey;
use hotshot_types::{
    data::{LeafType, ProposalType},
    traits::{
        network::TestableNetworkingImplementation,
        node_implementation::{NodeTypes, TestableNodeImplementation},
        signature_key::TestableSignatureKey,
        state::{TestableBlock, TestableState},
        storage::TestableStorage,
    },
    ExecutionType, HotShotConfig,
};
use std::{num::NonZeroUsize, time::Duration};

/// A launcher for [`TestRunner`], allowing you to customize the network and some default settings for spawning nodes.
pub struct TestLauncher<
    TYPES: NodeTypes,
    LEAF: LeafType<NodeType = TYPES>,
    PROPOSAL: ProposalType<NodeTypes = TYPES>,
    I: TestableNodeImplementation<TYPES>,
> where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState,
    TYPES::SignatureKey: TestableSignatureKey,
    I::Networking: TestableNetworkingImplementation<TYPES, LEAF, PROPOSAL>,
    I::Storage: TestableStorage<TYPES, LEAF>,
{
    pub(super) network: Generator<I::Networking>,
    pub(super) storage: Generator<I::Storage>,
    pub(super) block: Generator<TYPES::BlockType>,
    pub(super) config: HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
}

impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
        I: TestableNodeImplementation<TYPES>,
    > TestLauncher<TYPES, LEAF, PROPOSAL, I>
where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState,
    TYPES::SignatureKey: TestableSignatureKey,
    I::Networking: TestableNetworkingImplementation<TYPES, LEAF, PROPOSAL>,
    I::Storage: TestableStorage<TYPES, LEAF>,
{
    /// Create a new launcher.
    /// Note that `expected_node_count` should be set to an accurate value, as this is used to calculate the `threshold` internally.
    pub fn new(
        expected_node_count: usize,
        num_bootstrap_nodes: usize,
        min_transactions: usize,
        election_config: TYPES::ElectionConfigType,
    ) -> Self {
        let known_nodes = (0..expected_node_count)
            .map(|id| {
                let priv_key = TYPES::SignatureKey::generate_test_key(id as u64);
                TYPES::SignatureKey::from_private(&priv_key)
            })
            .collect();
        let config = HotShotConfig {
            execution_type: ExecutionType::Incremental,
            total_nodes: NonZeroUsize::new(expected_node_count).unwrap(),
            num_bootstrap: num_bootstrap_nodes,
            min_transactions,
            max_transactions: NonZeroUsize::new(99999).unwrap(),
            known_nodes,
            next_view_timeout: 500,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            propose_min_round_time: Duration::from_millis(0),
            propose_max_round_time: Duration::from_millis(1000),
            election_config: Some(election_config),
        };

        Self {
            network: I::Networking::generator(expected_node_count, num_bootstrap_nodes),
            storage: Box::new(|_| I::Storage::construct_tmp_storage().unwrap()),
            block: Box::new(|_| TYPES::BlockType::genesis()),
            config,
        }
    }
}

// TODO make these functions generic over the target networking/storage/other generics
// so we can hotswap out
impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
        I: TestableNodeImplementation<TYPES>,
    > TestLauncher<TYPES, LEAF, PROPOSAL, I>
where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState,
    TYPES::SignatureKey: TestableSignatureKey,
    I::Networking: TestableNetworkingImplementation<TYPES, LEAF, PROPOSAL>,
    I::Storage: TestableStorage<TYPES, LEAF>,
{
    /// Set a custom network generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_network(
        self,
        network: impl Fn(u64, TYPES::SignatureKey) -> I::Networking + 'static,
    ) -> TestLauncher<TYPES, LEAF, PROPOSAL, I> {
        TestLauncher {
            network: Box::new({
                move |node_id| {
                    // FIXME perhaps this pk generation is a separate function
                    // to add as an input
                    // that way we don't rely on threshold crypto
                    let priv_key = TYPES::SignatureKey::generate_test_key(node_id);
                    let pubkey = TYPES::SignatureKey::from_private(&priv_key);
                    network(node_id, pubkey)
                }
            }),
            storage: self.storage,
            block: self.block,
            config: self.config,
        }
    }

    /// Set a custom storage generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_storage(
        self,
        storage: impl Fn(u64) -> I::Storage + 'static,
    ) -> TestLauncher<TYPES, LEAF, PROPOSAL, I> {
        TestLauncher {
            network: self.network,
            storage: Box::new(storage),
            block: self.block,
            config: self.config,
        }
    }

    /// Set a custom block generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_block(
        self,
        block: impl Fn(u64) -> TYPES::BlockType + 'static,
    ) -> TestLauncher<TYPES, LEAF, PROPOSAL, I> {
        TestLauncher {
            network: self.network,
            storage: self.storage,
            block: Box::new(block),
            config: self.config,
        }
    }

    /// Set the default config of each node. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_default_config(
        mut self,
        config: HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> Self {
        self.config = config;
        self
    }

    /// Modifies the config used when generating nodes with `f`
    pub fn modify_default_config(
        mut self,
        mut f: impl FnMut(&mut HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>),
    ) -> Self {
        f(&mut self.config);
        self
    }
}

impl<
        TYPES: NodeTypes,
        LEAF: LeafType<NodeType = TYPES>,
        PROPOSAL: ProposalType<NodeTypes = TYPES>,
        I: TestableNodeImplementation<TYPES>,
    > TestLauncher<TYPES, LEAF, PROPOSAL, I>
where
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState,
    TYPES::SignatureKey: TestableSignatureKey,
    I::Networking: TestableNetworkingImplementation<TYPES, LEAF, PROPOSAL>,
    I::Storage: TestableStorage<TYPES, LEAF>,
{
    /// Launch the [`TestRunner`]. This function is only available if the following conditions are met:
    ///
    /// - `NETWORK` implements [`NetworkingImplementation`] and [`TestableNetworkingImplementation`]
    /// - `STORAGE` implements [`Storage`]
    /// - `BLOCK` implements [`BlockContents`] and [`TestableBlock`]
    pub fn launch(self) -> TestRunner<TYPES, LEAF, PROPOSAL, I> {
        TestRunner::new(self)
    }
}
