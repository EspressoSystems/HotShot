use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use hotshot::traits::{NodeImplementation, TestableNodeImplementation};
use hotshot_example_types::{
    auction_results_provider_types::TestAuctionResultsProvider, storage_types::TestStorage,
};
use hotshot_types::{
    traits::{
        network::{AsyncGenerator, ConnectedNetwork},
        node_implementation::NodeType,
    },
    HotShotConfig,
};

use super::{test_builder::TestDescription, test_runner::TestRunner};

/// A type alias to help readability
pub type Network<TYPES, I> = Arc<<I as NodeImplementation<TYPES>>::Network>;

/// Wrapper for a function that takes a `node_id` and returns an instance of `T`.
pub type Generator<T> = Box<dyn Fn(u64) -> T + 'static>;

/// generators for resources used by each node
pub struct ResourceGenerators<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    /// generate channels
    pub channel_generator: AsyncGenerator<Network<TYPES, I>>,
    /// generate new storage for each node
    pub storage: Generator<TestStorage<TYPES>>,
    /// configuration used to generate each hotshot node
    pub config: HotShotConfig<TYPES::SignatureKey>,
    /// generate a new auction results connector for each node
    pub auction_results_provider: Generator<TestAuctionResultsProvider>,
}

/// test launcher
pub struct TestLauncher<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> {
    /// generator for resources
    pub resource_generator: ResourceGenerators<TYPES, I>,
    /// metadasta used for tasks
    pub metadata: TestDescription,
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES>> TestLauncher<TYPES, I> {
    /// launch the test
    #[must_use]
    pub fn launch<N: ConnectedNetwork<TYPES::SignatureKey>>(self) -> TestRunner<TYPES, I, N> {
        TestRunner::<TYPES, I, N> {
            launcher: self,
            nodes: Vec::new(),
            servers: Vec::new(),
            late_start: HashMap::new(),
            next_node_id: 0,
            _pd: PhantomData,
        }
    }
    /// Modifies the config used when generating nodes with `f`
    #[must_use]
    pub fn modify_default_config(
        mut self,
        mut f: impl FnMut(&mut HotShotConfig<TYPES::SignatureKey>),
    ) -> Self {
        f(&mut self.resource_generator.config);
        self
    }
}
