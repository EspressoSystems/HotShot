use crate::round::{Round, RoundHook, RoundSafetyCheck, RoundSetup};
use crate::test_builder::{TestMetadata, TimingData};
use crate::test_runner::{
    CommitteeNetworkGenerator, Generator, QuorumNetworkGenerator, TestRunner,
    ViewSyncNetworkGenerator,
};
use hotshot::types::{Message, SignatureKey};
use hotshot::{traits::TestableNodeImplementation, HotShotType, SystemContext};
use hotshot_types::traits::election::{ConsensusExchange, Membership};
use hotshot_types::traits::node_implementation::{QuorumCommChannel, QuorumEx, QuorumNetwork};
use hotshot_types::{
    traits::{
        network::CommunicationChannel,
        node_implementation::{NodeImplementation, NodeType},
    },
    ExecutionType, HotShotConfig,
};
use std::{num::NonZeroUsize, time::Duration};
use hotshot_types::traits::signature_key::bn254::BN254Priv;
use jf_primitives::signatures::bls_over_bn254::{KeyPair as QCKeyPair, VerKey};
use hotshot_primitives::qc::bit_vector::StakeTableEntry;
use rand_chacha::rand_core::SeedableRng;
use rand_chacha::ChaCha20Rng;
use ethereum_types::U256;

/// generators for resources used by each node
pub struct ResourceGenerators<
    TYPES: NodeType,
    I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>,
> where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    /// generate the underlying quorum network used for each node
    pub network_generator: Generator<QuorumNetwork<TYPES, I>>,

    // TODO ED Make this a committee network; is a quorum network for now to get things working
    pub secondary_network_generator: Generator<QuorumNetwork<TYPES, I>>,
    /// generate a new quorum network for each node
    pub quorum_network: QuorumNetworkGenerator<TYPES, I, QuorumCommChannel<TYPES, I>>,
    /// generate a new committee network for each node
    // TODO ED Should be generic over any network, not quorum network specifically
    pub committee_network:
        CommitteeNetworkGenerator<QuorumNetwork<TYPES, I>, I::CommitteeCommChannel>,

    pub view_sync_network:
        ViewSyncNetworkGenerator<QuorumNetwork<TYPES, I>, I::ViewSyncCommChannel>,
    /// generate a new storage for each node
    pub storage: Generator<<I as NodeImplementation<TYPES>>::Storage>,
    /// configuration used to generate each hotshot node
    pub config: HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
}

/// A launcher for [`TestRunner`], allowing you to customize the network and some default settings for spawning nodes.
pub struct TestLauncher<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    pub generator: ResourceGenerators<TYPES, I>,
    // contains builder metadata that is used sporadically
    pub(super) metadata: TestMetadata,
    /// round information
    pub(super) round: Round<TYPES, I>,
}


impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
    TestLauncher<TYPES, I>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    
    /// Create a new launcher.
    /// Note that `expected_node_count` should be set to an accurate value, as this is used to calculate the `threshold` internally.
    pub fn new(metadata: TestMetadata, round: Round<TYPES, I>) -> Self
    where
        QuorumCommChannel<TYPES, I>: CommunicationChannel<
            TYPES,
            Message<TYPES, I>,
            <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
            <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
            <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
        >,
    {
        let TestMetadata {
            total_nodes,
            num_bootstrap_nodes,
            min_transactions,
            timing_data,
            da_committee_size,
            ..
        } = metadata;

        let known_nodes: Vec<<TYPES as NodeType>::SignatureKey> = (0..total_nodes)
            .map(|id| {
                let priv_key = TYPES::SignatureKey::generated_from_seed_indexed(
                    [0u8; 32],
                    id as u64,
                ).1;
                TYPES::SignatureKey::from_private(&priv_key)
            })
            .collect();
        let known_nodes_qc: Vec<StakeTableEntry<VerKey>> = (0..total_nodes)
        .map(|id| {
            let entry = StakeTableEntry {
                stake_key: known_nodes[id].get_internal_pub_key(),
                stake_amount: U256::from(1u8),
            };
            entry
        })
        .collect();

        let config = HotShotConfig {
            execution_type: ExecutionType::Incremental,
            total_nodes: NonZeroUsize::new(total_nodes).unwrap(),
            num_bootstrap: num_bootstrap_nodes,
            min_transactions,
            max_transactions: NonZeroUsize::new(99999).unwrap(),
            known_nodes,
            known_nodes_qc,
            da_committee_size: NonZeroUsize::new(da_committee_size).unwrap().into(),
            next_view_timeout: 500,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            propose_min_round_time: Duration::from_millis(0),
            propose_max_round_time: Duration::from_millis(1000),
            // TODO what's the difference between this and the second config?
            election_config: Some(<QuorumEx<TYPES, I> as ConsensusExchange<
                TYPES,
                Message<TYPES, I>,
            >>::Membership::default_election_config(
                total_nodes as u64
            )),
        };
        let TimingData {
            next_view_timeout,
            timeout_ratio,
            round_start_delay,
            start_delay,
            propose_min_round_time,
            propose_max_round_time,
        } = timing_data;

        let mod_config =
            // TODO this should really be using the timing config struct
            |a: &mut HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>| {
                a.next_view_timeout = next_view_timeout;
                a.timeout_ratio = timeout_ratio;
                a.round_start_delay = round_start_delay;
                a.start_delay = start_delay;
                a.propose_min_round_time = propose_min_round_time;
                a.propose_max_round_time = propose_max_round_time;
            };

        // TODO ED This should call a secondary_network_generator function in the future
        let network_generator =
            I::network_generator(total_nodes, num_bootstrap_nodes, da_committee_size, false);
        let secondary_network_generator =
            I::network_generator(total_nodes, num_bootstrap_nodes, da_committee_size, true);
        Self {
            generator: ResourceGenerators {
                network_generator,
                secondary_network_generator,
                quorum_network: I::quorum_comm_channel_generator(),
                committee_network: I::committee_comm_channel_generator(),
                view_sync_network: I::view_sync_comm_channel_generator(),

                storage: Box::new(|_| I::construct_tmp_storage().unwrap()),
                config,
            },
            metadata,
            round,
        }
        .modify_default_config(mod_config)
    }
}

// TODO make these functions generic over the target networking/storage/other generics
// so we can hotswap out
impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
    TestLauncher<TYPES, I>
where
    QuorumCommChannel<TYPES, I>: CommunicationChannel<
        TYPES,
        Message<TYPES, I>,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Proposal,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Vote,
        <QuorumEx<TYPES, I> as ConsensusExchange<TYPES, Message<TYPES, I>>>::Membership,
    >,
{
    /// Set a custom storage generator. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_storage(
        mut self,
        storage: impl Fn(u64) -> I::Storage + 'static,
    ) -> TestLauncher<TYPES, I> {
        self.generator.storage = Box::new(storage);
        self
    }

    /// directly override the round information
    pub fn with_round(self, round: Round<TYPES, I>) -> TestLauncher<TYPES, I> {
        TestLauncher { round, ..self }
    }

    /// directly override hooks
    pub fn with_hooks(self, hooks: Vec<RoundHook<TYPES, I>>) -> Self {
        let round = self.round.clone();
        TestLauncher {
            round: Round { hooks, ..round },
            ..self
        }
    }

    /// push a hook
    pub fn push_hook(self, hook: RoundHook<TYPES, I>) -> Self {
        let round = self.round.clone();
        let mut hooks = round.hooks.clone();
        hooks.push(hook);

        TestLauncher {
            round: Round { hooks, ..round },
            ..self
        }
    }

    /// directly override safety check
    pub fn with_safety_check(self, safety_check: RoundSafetyCheck<TYPES, I>) -> Self {
        let round = self.round.clone();
        TestLauncher {
            round: Round {
                safety_check,
                ..round
            },
            ..self
        }
    }

    /// directly override hooks
    pub fn with_setup(self, setup_round: RoundSetup<TYPES, I>) -> Self {
        let round = self.round.clone();
        TestLauncher {
            round: Round {
                setup_round,
                ..round
            },
            ..self
        }
    }

    /// Set the default config of each node. Note that this can also be overwritten per-node in the [`TestLauncher`].
    pub fn with_default_config(
        mut self,
        config: HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>,
    ) -> Self {
        self.generator.config = config;
        self
    }

    /// Modifies the config used when generating nodes with `f`
    pub fn modify_default_config(
        mut self,
        mut f: impl FnMut(&mut HotShotConfig<TYPES::SignatureKey, TYPES::ElectionConfigType>),
    ) -> Self {
        f(&mut self.generator.config);
        self
    }
}

impl<TYPES: NodeType, I: TestableNodeImplementation<TYPES::ConsensusType, TYPES>>
    TestLauncher<TYPES, I>
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
    /// Launch the [`TestRunner`]. This function is only available if the following conditions are met:
    ///
    /// - `NETWORK` implements and [`hotshot_types::traits::network::TestableNetworkingImplementation`]
    /// - `STORAGE` implements [`hotshot::traits::Storage`]
    /// - `BLOCK` implements [`hotshot::traits::Block`] and [`hotshot_types::traits::state::TestableBlock`]
    pub fn launch(self) -> TestRunner<TYPES, I> {
        TestRunner::new(self)
    }
}
