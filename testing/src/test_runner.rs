use nll::nll_todo::nll_todo;
use std::{collections::HashMap, sync::Arc};

use crate::{
    round::{Round, RoundCtx, RoundResult},
    test_errors::ConsensusTestError,
    test_launcher::TestLauncher,
};
use async_compatibility_layer::logging::{setup_backtrace, setup_logging};
use console_subscriber;
use hotshot::{
    traits::{NodeImplementation, TestableNodeImplementation},
    types::{Message, SystemContextHandle},
    HotShotError, HotShotInitializer, HotShotType, SystemContext, ViewRunner,
};
use hotshot_types::{
    certificate::QuorumCertificate,
    traits::{
        election::ConsensusExchange,
        election::Membership,
        metrics::NoMetrics,
        network::CommunicationChannel,
        node_implementation::{
            ExchangesType, NodeType, QuorumCommChannel, QuorumEx, QuorumNetwork,
        },
        signature_key::SignatureKey,
    },
    HotShotConfig,
};
use tracing::{debug, info, warn};

/// Wrapper for a function that takes a `node_id` and returns an instance of `T`.
pub type Generator<T> = Box<dyn Fn(u64) -> T + 'static>;

/// Wrapper Type for quorum function that takes a `ConnectedNetwork` and returns a `CommunicationChannel`
pub type QuorumNetworkGenerator<TYPES, I, T> =
    Box<dyn Fn(Arc<QuorumNetwork<TYPES, I>>) -> T + 'static>;

/// Wrapper Type for committee function that takes a `ConnectedNetwork` and returns a `CommunicationChannel`
pub type CommitteeNetworkGenerator<N, T> = Box<dyn Fn(Arc<N>) -> T + 'static>;

pub type ViewSyncNetworkGenerator<N, T> = Box<dyn Fn(Arc<N>) -> T + 'static>;

/// The runner of a test network
/// spin up and down nodes, execute rounds
pub struct TestRunner<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    launcher: TestLauncher<TYPES, I>,
    nodes: Vec<Node<TYPES, I>>,
    next_node_id: u64,
}

#[derive(Clone)]
pub struct Node<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>> {
    pub node_id: u64,
    pub handle: SystemContextHandle<TYPES, I>,
}

/// HACK we want a concise and a wordy way to print things
/// unfortunately, debug is only available for option
/// and display is not
#[allow(clippy::type_complexity)]
pub fn concise_leaf_and_node<
    TYPES: NodeType,
    I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
>(
    result: &Result<
        (
            Vec<<I as NodeImplementation<TYPES>>::Leaf>,
            QuorumCertificate<TYPES, <I as NodeImplementation<TYPES>>::Leaf>,
        ),
        HotShotError<TYPES>,
    >,
) -> String {
    match result {
        Ok(ok) => {
            let mut rstring = "vec![".to_string();
            for leaf in &ok.0 {
                rstring.push_str(&format!("{}", leaf));
            }
            rstring.push(']');

            format!("Ok(({}, {}))", rstring, ok.1)
        }
        Err(err) => {
            format!("Err({})", err)
        }
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
    TestRunner<TYPES, I>
where
    SystemContext<TYPES::ConsensusType, TYPES, I>: HotShotType<TYPES, I>,
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    pub(crate) fn new(launcher: TestLauncher<TYPES, I>) -> Self {
        Self {
            nodes: Vec::new(),
            next_node_id: 0,
            launcher,
        }
    }

    /// return the number of juccesses needed
    pub fn num_succeeds(&self) -> usize {
        self.launcher.metadata.num_succeeds
    }

    /// run the test
    pub async fn run_test(mut self) -> Result<(), ConsensusTestError>
    where
        SystemContext<TYPES::ConsensusType, TYPES, I>: ViewRunner<TYPES, I>,
        I::Exchanges: ExchangesType<
            TYPES::ConsensusType,
            TYPES,
            I::Leaf,
            Message<TYPES, I>,
            Networks = (
                QuorumCommChannel<TYPES, I>,
                I::ViewSyncCommChannel,
                I::CommitteeCommChannel,
            ),
            ElectionConfigs = (TYPES::ElectionConfigType, I::CommitteeElectionConfig),
        >,
    {
        setup_logging();
        setup_backtrace();

        // configure nodes/timing
        self.add_nodes(self.launcher.metadata.start_nodes).await;

        for (idx, node) in self.nodes().collect::<Vec<_>>().iter().enumerate().rev() {
            node.wait_for_networks_ready().await;
            info!("EXECUTOR: NODE {:?} IS READY", idx);
        }

        self.execute_rounds(
            self.launcher.metadata.num_succeeds,
            self.launcher.metadata.failure_threshold,
        )
        .await
        .unwrap();

        Ok(())
    }

    /// Add `count` nodes to the network. These will be spawned with the default node config and state
    pub async fn add_nodes(&mut self, count: usize) -> Vec<u64>
    where
        SystemContext<TYPES::ConsensusType, TYPES, I>: ViewRunner<TYPES, I>,
        I::Exchanges: ExchangesType<
            TYPES::ConsensusType,
            TYPES,
            I::Leaf,
            Message<TYPES, I>,
            Networks = (
                QuorumCommChannel<TYPES, I>,
                I::ViewSyncCommChannel,
                I::CommitteeCommChannel,
            ),
            ElectionConfigs = (TYPES::ElectionConfigType, I::CommitteeElectionConfig),
        >,
    {

        let mut results = vec![];
        for _i in 0..count {
            tracing::error!("running node{}", _i);
            let node_id = self.next_node_id;
            let network_generator = Arc::new((self.launcher.generator.network_generator)(node_id));

            // NOTE ED: This creates a secondary network for the committee network.  As of now this always creates a secondary network,
            // so libp2p tests will not work since they are not configured to have two running at the same time.  If you want to
            // test libp2p commout out the below lines where noted.

            // NOTE ED: Comment out this line to run libp2p tests
            let secondary_network_generator =
                Arc::new((self.launcher.generator.secondary_network_generator)(
                    node_id,
                ));

            let quorum_network =
                (self.launcher.generator.quorum_network)(network_generator.clone());
            let committee_network =
                (self.launcher.generator.committee_network)(secondary_network_generator);
            // NOTE ED: Switch the below line with the above line to run libp2p tests
            // let committee_network = (self.launcher.generator.committee_network)(network_generator);
            let storage = (self.launcher.generator.storage)(node_id);
            let config = self.launcher.generator.config.clone();
            let initializer =
                HotShotInitializer::<TYPES, I::Leaf>::from_genesis(I::block_genesis()).unwrap();
            let node_id = self
                .add_node_with_config(
                    quorum_network,
                    committee_network,
                    storage,
                    initializer,
                    config,
                )
                .await;
            results.push(node_id);
        }

        results
    }

    /// replace round list
    #[allow(clippy::type_complexity)]
    pub fn with_round(&mut self, round: Round<TYPES, I>) {
        self.launcher.round = round;
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
        quorum_network: QuorumCommChannel<TYPES, I>,
        committee_network: I::CommitteeCommChannel,
        storage: I::Storage,
        initializer: HotShotInitializer<TYPES, I::Leaf>,
        config: HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> u64
    where
        SystemContext<TYPES::ConsensusType, TYPES, I>: ViewRunner<TYPES, I>,
        I::Exchanges: ExchangesType<
            TYPES::ConsensusType,
            TYPES,
            I::Leaf,
            Message<TYPES, I>,
            Networks = (
                QuorumCommChannel<TYPES, I>,
                I::ViewSyncCommChannel,
                I::CommitteeCommChannel,
            ),
            ElectionConfigs = (TYPES::ElectionConfigType, I::CommitteeElectionConfig),
        >,
    {
        let node_id = self.next_node_id;
        self.next_node_id += 1;

        let known_nodes = config.known_nodes.clone();
        let known_nodes_with_stake = config.known_nodes_with_stake.clone();
        // Generate key pair for certificate aggregation
        let private_key = TYPES::SignatureKey::generated_from_seed_indexed(
            [0u8; 32],
            node_id,
        ).1;
        let public_key = TYPES::SignatureKey::from_private(&private_key);
        let entry =  public_key.get_stake_table_entry(1u64);
        let quorum_election_config = config.election_config.clone().unwrap_or_else(|| {
            <QuorumEx<TYPES,I> as ConsensusExchange<
                TYPES,
                Message<TYPES, I>,
            >>::Membership::default_election_config(config.total_nodes.get() as u64)
        });

        let committee_election_config = I::committee_election_config_generator();

        let exchanges = I::Exchanges::create(
            known_nodes_with_stake.clone(),
            known_nodes.clone(),
            (
                quorum_election_config,
                committee_election_config(config.da_committee_size as u64),
            ),
            // TODO ED Add view sync network here
            (quorum_network, nll_todo(), committee_network),
            public_key.clone(),
            entry.clone(),
            private_key.clone(),
        );
        let handle = SystemContext::init(
            public_key,
            private_key,
            node_id,
            config,
            storage,
            exchanges,
            initializer,
            NoMetrics::boxed(),
        )
        .await
        .expect("Could not init hotshot");
        self.nodes.push(Node { handle, node_id });
        node_id
    }

    /// Iterate over the [`SystemContextHandle`] nodes in this runner.
    pub fn nodes(&self) -> impl Iterator<Item = &SystemContextHandle<TYPES, I>> + '_ {
        self.nodes.iter().map(|node| &node.handle)
    }

    /// repeatedly executes consensus until either:
    /// * `self.fail_threshold` rounds fail
    /// * `self.num_succeeds` rounds are successful
    /// (for a definition of success defined by safety checks)
    pub async fn execute_rounds(
        &mut self,
        _num_success: usize,
        _fail_threshold: usize,
    ) -> Result<(), ConsensusTestError> {
        // the default context starts as empty
        let mut ctx = RoundCtx::<TYPES, I>::default();
        loop {
            if let Err(e) = self.execute_round(&mut ctx).await {
                match e {
                    ConsensusTestError::CompletedTestSuccessfully => return Ok(()),
                    e => {
                        panic!(
                            "TEST FAILED WITH Err: {e:#?}, \n TEST FAILED WITH context: {ctx:#?}"
                        )
                    }
                }
            }
        }
    }

    /// Execute a single round of consensus
    /// This consists of the following steps:
    /// - checking the state of the hotshot
    /// - setting up the round (ex: submitting txns) or spinning up or down nodes
    /// - checking safety conditions to ensure that the round executed as expected
    pub async fn execute_round(
        &mut self,
        ctx: &mut RoundCtx<TYPES, I>,
    ) -> Result<(), ConsensusTestError> {
        let Round {
            hooks,
            setup_round,
            safety_check,
        } = self.launcher.round.clone();

        info!("RUNNING HOOK");
        for hook in hooks {
            hook(self, ctx).await?;
        }

        info!("RUNNING SETUP");
        let txns = setup_round(self, ctx).await;
        info!("RUNNING VIEW");
        let results = self.run_one_round(txns).await;
        info!("RUNNING SAFETY");
        safety_check(self, ctx, results).await?;
        Ok(())
    }

    /// Internal function that unpauses hotshots and waits for round to complete,
    /// returns a `RoundResult` upon successful completion, indicating what (if anything) was
    /// committed
    async fn run_one_round(
        &mut self,
        txns: Vec<TYPES::Transaction>,
    ) -> RoundResult<TYPES, I::Leaf> {
        let mut results = HashMap::new();

        info!("EXECUTOR: running one round");
        for handle in self.nodes() {
            info!("STARTING ONE ROUND");
            // TODO (justin) fix this. we need long running tasks
            let _: () = nll_todo();
            // handle.start_one_round().await;
        }
        info!("EXECUTOR: done running one round");
        let mut failures = HashMap::new();
        for node in &mut self.nodes {
            info!("EXECUTOR: COLLECTING NODE {:?}", node.node_id.clone());
            // let result = node.handle.collect_round_events().await;
            // TODO (justin) fix this. we need long running tasks
            let result = nll_todo();
            info!(
                "EXECUTOR: collected node {:?} results: {}",
                node.node_id.clone(),
                concise_leaf_and_node::<TYPES, I>(&result)
            );
            match result {
                Ok(leaves) => {
                    results.insert(node.node_id, leaves);
                }
                Err(e) => {
                    failures.insert(node.node_id, e);
                }
            }
        }
        info!("All nodes reached decision");
        if !failures.is_empty() {
            warn!(
                "Some failures this round. Failing nodes: {:?}. Successful nodes: {:?}",
                failures, results
            );
        }
        RoundResult {
            txns,
            success_nodes: results,
            failed_nodes: failures,
            /// setting this to success, It's the repsonsibiity of the checks and hooks
            /// to mark and report this as a failure
            success: Ok(()),
            agreed_state: None,
            agreed_block: None,
            agreed_leaf: None,
        }
    }

    /// Gracefully shut down this system
    pub async fn shutdown_all(&mut self) {
        for node in &mut self.nodes {
            node.handle.shut_down().await;
        }
        debug!("All nodes should be shut down now.");
    }

    /// In-place shut down an individual node with id `node_id`
    /// # Errors
    /// returns [`ConsensusRoundError::NoSuchNode`] if the node idx is either
    /// - already shut down
    /// - does not exist
    pub async fn shutdown(&mut self, node_id: u64) -> Result<(), ConsensusTestError> {
        let maybe_idx = self.nodes.iter().position(|n| n.node_id == node_id);
        if let Some(idx) = maybe_idx {
            let mut node = self.nodes.remove(idx);
            node.handle.shut_down().await;
            Ok(())
        } else {
            Err(ConsensusTestError::NoSuchNode {
                node_ids: self.ids(),
                requested_id: node_id,
            })
        }
    }

    /// returns the requested handle specified by `id` if it exists
    /// else returns `None`
    pub fn get_handle(&self, id: u64) -> Option<SystemContextHandle<TYPES, I>> {
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

    /// Add a random transaction to this runner.
    pub async fn add_random_transaction(
        &self,
        node_id: Option<usize>,
        rng: &mut dyn rand::RngCore,
    ) -> TYPES::Transaction {
        if self.nodes.is_empty() {
            panic!("Tried to add transaction, but no nodes have been added!");
        }

        use rand::seq::IteratorRandom;

        // we're assuming all nodes have the same leaf.
        // If they don't match, this is probably fine since
        // it should be caught by an assertion (and the txn will be rejected anyway)
        let leaf = self.nodes[0].handle.get_decided_leaf().await;

        let txn = I::leaf_create_random_transaction(&leaf, rng, 0);

        let node = if let Some(node_id) = node_id {
            self.nodes.get(node_id).unwrap()
        } else {
            // find a random handle to send this transaction from
            self.nodes.iter().choose(rng).unwrap()
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
        rng: &mut dyn rand::RngCore,
    ) -> Option<Vec<TYPES::Transaction>> {
        let mut result = Vec::new();
        for _ in 0..n {
            result.push(self.add_random_transaction(None, rng).await);
        }
        Some(result)
    }
}

#[cfg(test)]
pub mod test {
    use ark_bls12_381::Parameters as Param381;
    use hotshot::{
        demos::sdemo::{SDemoBlock, SDemoState, SDemoTransaction},
        traits::{
            election::{
                static_committee::{StaticCommittee, StaticElectionConfig, StaticVoteToken},
            },
            implementations::{
                CentralizedCommChannel, Libp2pCommChannel, MemoryCommChannel, MemoryStorage,
                WebCommChannel,
            },
            NodeImplementation,
        }, types::bn254::BN254Pub,
    };
    use hotshot_types::message::{Message, SequencingMessage};
    use hotshot_types::traits::election::ViewSyncExchange;
    use hotshot_types::vote::QuorumVote;
    use hotshot_types::vote::ViewSyncVote;
    use hotshot_types::{certificate::ViewSyncCertificate, data::QuorumProposal};
    use hotshot_types::{
        data::{DAProposal, SequencingLeaf, ViewNumber},
        traits::{
            consensus_type::sequencing_consensus::SequencingConsensus,
            election::{CommitteeExchange, QuorumExchange},
            node_implementation::{ChannelMaps, NodeType, SequencingExchanges},
        },
        vote::DAVote,
    };
    use serde::{Deserialize, Serialize};
    use tracing::instrument;
    #[derive(
        Copy,
        Clone,
        Debug,
        Default,
        Hash,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        serde::Serialize,
        serde::Deserialize,
    )]
    pub struct SequencingTestTypes;
    impl NodeType for SequencingTestTypes {
        type ConsensusType = SequencingConsensus;
        type Time = ViewNumber;
        type BlockType = SDemoBlock;
        type SignatureKey = BN254Pub; 
        type VoteTokenType = StaticVoteToken<Self::SignatureKey>;
        type Transaction = SDemoTransaction;
        type ElectionConfigType = StaticElectionConfig;
        type StateType = SDemoState;
    }

    #[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
    pub struct SequencingMemoryImpl {}

    type StaticMembership =
        StaticCommittee<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;

    type StaticDAComm = MemoryCommChannel<
        SequencingTestTypes,
        SequencingMemoryImpl,
        DAProposal<SequencingTestTypes>,
        DAVote<SequencingTestTypes>,
        StaticMembership,
    >;

    type StaticQuroumComm = MemoryCommChannel<
        SequencingTestTypes,
        SequencingMemoryImpl,
        QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
        QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
        StaticMembership,
    >;

    type StaticViewSyncComm = MemoryCommChannel<
        SequencingTestTypes,
        SequencingMemoryImpl,
        ViewSyncCertificate<SequencingTestTypes>,
        ViewSyncVote<SequencingTestTypes>,
        StaticMembership,
    >;

    impl NodeImplementation<SequencingTestTypes> for SequencingMemoryImpl {
        type Storage = MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;
        type Leaf = SequencingLeaf<SequencingTestTypes>;
        type Exchanges = SequencingExchanges<
            SequencingTestTypes,
            Message<SequencingTestTypes, Self>,
            QuorumExchange<
                SequencingTestTypes,
                Self::Leaf,
                QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
                StaticMembership,
                StaticQuroumComm,
                Message<SequencingTestTypes, Self>,
            >,
            CommitteeExchange<
                SequencingTestTypes,
                StaticMembership,
                StaticDAComm,
                Message<SequencingTestTypes, Self>,
            >,
            ViewSyncExchange<
                SequencingTestTypes,
                ViewSyncCertificate<SequencingTestTypes>,
                StaticMembership,
                StaticViewSyncComm,
                Message<SequencingTestTypes, Self>,
            >,
        >;
        type ConsensusMessage = SequencingMessage<SequencingTestTypes, Self>;

        fn new_channel_maps(
            start_view: ViewNumber,
        ) -> (
            ChannelMaps<SequencingTestTypes, Self>,
            Option<ChannelMaps<SequencingTestTypes, Self>>,
        ) {
            (
                ChannelMaps::new(start_view),
                Some(ChannelMaps::new(start_view)),
            )
        }
    }

    #[cfg(test)]
    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    async fn test_basic() {
        async_compatibility_layer::logging::setup_logging();
        async_compatibility_layer::logging::setup_backtrace();
        let metadata = crate::app_tasks::test_builder::TestMetadata::default();
        metadata
            .gen_launcher::<SequencingTestTypes, SequencingMemoryImpl>()
            .launch()
            .run_test()
            .await
            .unwrap();
    }

    #[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
    pub struct SequencingWebImpl {}

    type StaticWebDAComm = WebCommChannel<
        SequencingTestTypes,
        SequencingWebImpl,
        DAProposal<SequencingTestTypes>,
        DAVote<SequencingTestTypes>,
        StaticMembership,
    >;

    type StaticWebQuroumComm = WebCommChannel<
        SequencingTestTypes,
        SequencingWebImpl,
        QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
        QuorumVote<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
        StaticMembership,
    >;

    type StaticWebViewSyncComm = WebCommChannel<
        SequencingTestTypes,
        SequencingWebImpl,
        ViewSyncCertificate<SequencingTestTypes>,
        ViewSyncVote<SequencingTestTypes>,
        StaticMembership,
    >;

    impl NodeImplementation<SequencingTestTypes> for SequencingWebImpl {
        type Storage = MemoryStorage<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>;
        type Leaf = SequencingLeaf<SequencingTestTypes>;
        type Exchanges = SequencingExchanges<
            SequencingTestTypes,
            Message<SequencingTestTypes, Self>,
            QuorumExchange<
                SequencingTestTypes,
                Self::Leaf,
                QuorumProposal<SequencingTestTypes, SequencingLeaf<SequencingTestTypes>>,
                StaticMembership,
                StaticWebQuroumComm,
                Message<SequencingTestTypes, Self>,
            >,
            CommitteeExchange<
                SequencingTestTypes,
                StaticMembership,
                StaticWebDAComm,
                Message<SequencingTestTypes, Self>,
            >,
            ViewSyncExchange<
                SequencingTestTypes,
                ViewSyncCertificate<SequencingTestTypes>,
                StaticMembership,
                StaticWebViewSyncComm,
                Message<SequencingTestTypes, Self>,
            >,
        >;
        type ConsensusMessage = SequencingMessage<SequencingTestTypes, Self>;

        fn new_channel_maps(
            start_view: ViewNumber,
        ) -> (
            ChannelMaps<SequencingTestTypes, Self>,
            Option<ChannelMaps<SequencingTestTypes, Self>>,
        ) {
            (
                ChannelMaps::new(start_view),
                Some(ChannelMaps::new(start_view)),
            )
        }
    }

    #[cfg(test)]
    #[cfg_attr(
        feature = "tokio-executor",
        tokio::test(flavor = "multi_thread", worker_threads = 2)
    )]
    #[cfg_attr(feature = "async-std-executor", async_std::test)]
    async fn test_basic_web_server() {
        // console_subscriber::init();
        async_compatibility_layer::logging::setup_logging();
        async_compatibility_layer::logging::setup_backtrace();
        let metadata = crate::app_tasks::test_builder::TestMetadata::default();
        metadata
            .gen_launcher::<SequencingTestTypes, SequencingWebImpl>()
            .launch()
            .run_test()
            .await
            .unwrap();
    }
}
