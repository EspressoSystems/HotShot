//! This crate contains examples of HotShot usage
use std::{collections::HashMap, num::NonZero, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use cdn_broker::{reexports::def::hook::NoMessageHook, Broker, Config as BrokerConfig};
use cdn_marshal::{Config as MarshalConfig, Marshal};
use clap::Parser;
use futures::StreamExt;
use hotshot::{
    helpers::initialize_logging,
    traits::{
        election::static_committee::StaticCommittee,
        implementations::{
            derive_libp2p_keypair, CdnMetricsValue, CdnTopic, CombinedNetworks, KeyPair,
            Libp2pMetricsValue, Libp2pNetwork, PushCdnNetwork, TestingDef, WrappedSignatureKey,
        },
    },
    types::{BLSPrivKey, BLSPubKey, EventType, SignatureKey},
    MarketplaceConfig, SystemContext,
};
use hotshot_example_types::{
    auction_results_provider_types::TestAuctionResultsProvider,
    block_types::TestTransaction,
    node_types::{CombinedImpl, Libp2pImpl, PushCdnImpl, TestTypes, TestVersions},
    state_types::TestInstanceState,
    storage_types::TestStorage,
    testable_delay::DelayConfig,
};
use hotshot_testing::block_builder::{SimpleBuilderImplementation, TestBuilderImplementation};
use hotshot_types::{
    consensus::ConsensusMetricsValue,
    light_client::StateKeyPair,
    traits::{election::Membership, node_implementation::NodeType},
    HotShotConfig, PeerConfig,
};
use libp2p::Multiaddr;
use libp2p_networking::network::{
    behaviours::dht::record::{Namespace, RecordKey, RecordValue},
    node::config::{KademliaConfig, Libp2pConfig},
    GossipConfig, RequestResponseConfig,
};
use lru::LruCache;
use rand::Rng;
use tokio::{spawn, sync::OnceCell, task::JoinSet};
use tracing::{error, info};
use url::Url;

/// The command line arguments for the example
#[derive(Parser)]
struct Args {
    /// The number of nodes to start
    #[arg(long, default_value_t = 5)]
    total_num_nodes: usize,

    /// The number of nodes which are DA nodes
    #[arg(long, default_value_t = 3)]
    num_da_nodes: usize,

    /// The number of views to run for. If not specified, it will run indefinitely
    #[arg(long)]
    num_views: Option<usize>,

    /// The number of transactions to submit to the builder per view
    #[arg(long, default_value_t = 100)]
    num_transactions_per_view: usize,

    /// The size of the transactions submitted to the builder per view
    #[arg(long, default_value_t = 1000)]
    transaction_size: usize,

    /// The type of network to use. Acceptable values are
    /// "combined", "cdn", or "libp2p"
    #[arg(long, default_value = "combined")]
    network: String,
}

/// This is a testing function which allows us to easily determine if a node should be a DA node
fn is_da_node(index: usize, num_da_nodes: usize) -> bool {
    index < num_da_nodes
}

/// This is a testing function which allows us to easily generate peer configs from indexes
fn peer_info_from_index(index: usize) -> PeerConfig<BLSPubKey> {
    // Get the node's public key
    let (public_key, _) = BLSPubKey::generated_from_seed_indexed([0u8; 32], index as u64);

    // Generate the peer config
    PeerConfig {
        stake_table_entry: public_key.stake_table_entry(1),
        state_ver_key: StateKeyPair::default().0.ver_key(),
    }
}

/// Generate a Libp2p multiaddress from a port
fn libp2p_multiaddress_from_index(index: usize) -> Result<Multiaddr> {
    // Generate the peer's private key and derive their libp2p keypair
    let (_, peer_private_key) = BLSPubKey::generated_from_seed_indexed([0u8; 32], index as u64);
    let peer_libp2p_keypair = derive_libp2p_keypair::<BLSPubKey>(&peer_private_key)
        .with_context(|| "Failed to derive libp2p keypair")?;

    // Generate the multiaddress from the peer's port and libp2p keypair
    format!(
        "/ip4/127.0.0.1/udp/{}/quic-v1/p2p/{}",
        portpicker::pick_unused_port().with_context(|| "Failed to find unused port")?,
        peer_libp2p_keypair.public().to_peer_id()
    )
    .parse()
    .with_context(|| "Failed to parse multiaddress")
}

/// The type of network to use for the example
#[derive(Debug, PartialEq, Eq)]
enum NetworkType {
    /// A combined network, which is a combination of a Libp2p and Push CDN network
    Combined,

    /// A network solely using the Push CDN
    Cdn,

    /// A Libp2p network
    LibP2P,
}

/// A helper function to start the CDN
/// (2 brokers + 1 marshal)
///
/// Returns the address of the marshal
async fn start_cdn() -> Result<String> {
    // Figure out where we're going to spawn the marshal
    let marshal_port =
        portpicker::pick_unused_port().with_context(|| "Failed to find unused port")?;
    let marshal_address = format!("127.0.0.1:{marshal_port}");

    // Configure the marshal
    let marshal_config = MarshalConfig {
        bind_endpoint: marshal_address.clone(),
        discovery_endpoint: marshal_address.clone(),
        ca_cert_path: None,
        ca_key_path: None,
        metrics_bind_endpoint: None,
        global_memory_pool_size: Some(1024 * 1024 * 1024),
    };

    // Create and start the marshal
    let marshal: Marshal<TestingDef<BLSPubKey>> = Marshal::new(marshal_config)
        .await
        .with_context(|| "Failed to create marshal")?;
    spawn(marshal.start());

    // This keypair is shared between brokers
    let (broker_public_key, broker_private_key) =
        <TestTypes as NodeType>::SignatureKey::generated_from_seed_indexed([0u8; 32], 1337);

    for _ in 0..2 {
        // Generate one random port for the "public" endpoint and one for the "private" endpoint
        let public_port =
            portpicker::pick_unused_port().with_context(|| "Failed to find unused port")?;
        let private_port =
            portpicker::pick_unused_port().with_context(|| "Failed to find unused port")?;

        // Throw into address format
        let private_address = format!("127.0.0.1:{private_port}");
        let public_address = format!("127.0.0.1:{public_port}");

        // Configure the broker
        let broker_config: BrokerConfig<TestingDef<<TestTypes as NodeType>::SignatureKey>> =
            BrokerConfig {
                public_advertise_endpoint: public_address.clone(),
                public_bind_endpoint: public_address,
                private_advertise_endpoint: private_address.clone(),
                private_bind_endpoint: private_address,
                discovery_endpoint: marshal_address.clone(),
                keypair: KeyPair {
                    public_key: WrappedSignatureKey(broker_public_key),
                    private_key: broker_private_key.clone(),
                },

                user_message_hook: NoMessageHook, // Don't do any message processing
                broker_message_hook: NoMessageHook,

                metrics_bind_endpoint: None,
                ca_cert_path: None,
                ca_key_path: None,
                global_memory_pool_size: Some(1024 * 1024 * 1024),
            };

        // Create and start it
        let broker = Broker::new(broker_config)
            .await
            .with_context(|| "Failed to create broker")?;
        spawn(broker.start());
    }

    Ok(marshal_address)
}

/// A helper function to create a Libp2p network
async fn create_libp2p_network(
    node_index: usize,
    total_num_nodes: usize,
    public_key: &BLSPubKey,
    private_key: &BLSPrivKey,
    known_libp2p_nodes: &[Multiaddr],
) -> Result<Arc<Libp2pNetwork<TestTypes>>> {
    // Derive the Libp2p keypair from the private key
    let libp2p_keypair = derive_libp2p_keypair::<BLSPubKey>(private_key)
        .with_context(|| "Failed to derive libp2p keypair")?;

    // Sign our Libp2p lookup record value
    let lookup_record_value = RecordValue::new_signed(
        &RecordKey::new(Namespace::Lookup, public_key.to_bytes()),
        libp2p_keypair.public().to_peer_id().to_bytes(),
        private_key,
    )
    .expect("Failed to sign DHT lookup record");

    // Configure Libp2p
    let libp2p_config = Libp2pConfig {
        keypair: libp2p_keypair,
        bind_address: known_libp2p_nodes[node_index].clone(),
        known_peers: known_libp2p_nodes.to_vec(),
        quorum_membership: None, // This disables stake-table authentication
        auth_message: None,      // This disables stake-table authentication
        gossip_config: GossipConfig::default(),
        request_response_config: RequestResponseConfig::default(),
        kademlia_config: KademliaConfig {
            replication_factor: total_num_nodes * 2 / 3,
            record_ttl: None,
            publication_interval: None,
            file_path: format!("/tmp/kademlia-{}.db", rand::random::<u32>()),
            lookup_record_value,
        },
    };

    // Create the network with the config
    Ok(Arc::new(
        Libp2pNetwork::new(
            libp2p_config,
            public_key,
            Libp2pMetricsValue::default(),
            None,
        )
        .await
        .with_context(|| "Failed to create libp2p network")?,
    ))
}

/// Create a Push CDN network, starting the CDN if it's not already running
async fn create_cdn_network(
    is_da_node: bool,
    public_key: &BLSPubKey,
    private_key: &BLSPrivKey,
) -> Result<Arc<PushCdnNetwork<BLSPubKey>>> {
    /// Create a static cell to store the marshal's endpoint
    static MARSHAL_ENDPOINT: OnceCell<String> = OnceCell::const_new();

    // If the marshal endpoint isn't already set, start the CDN and set the endpoint
    let marshal_endpoint = MARSHAL_ENDPOINT
        .get_or_init(|| async { start_cdn().await.expect("Failed to start CDN") })
        .await
        .clone();

    // Subscribe to topics based on whether we're a DA node or not
    let mut topics = vec![CdnTopic::Global];
    if is_da_node {
        topics.push(CdnTopic::Da);
    }

    // Create and return the network
    Ok(Arc::new(
        PushCdnNetwork::new(
            marshal_endpoint,
            topics,
            KeyPair {
                public_key: WrappedSignatureKey(*public_key),
                private_key: private_key.clone(),
            },
            CdnMetricsValue::default(),
        )
        .with_context(|| "Failed to create Push CDN network")?,
    ))
}

/// A helper function to create a Combined network, which is a combination of a Libp2p and Push CDN network
async fn create_combined_network(
    node_index: usize,
    total_num_nodes: usize,
    known_libp2p_nodes: &[Multiaddr],
    is_da_node: bool,
    public_key: &BLSPubKey,
    private_key: &BLSPrivKey,
) -> Result<Arc<CombinedNetworks<TestTypes>>> {
    // Create the CDN network
    let cdn_network =
        Arc::into_inner(create_cdn_network(is_da_node, public_key, private_key).await?).unwrap();

    // Create the Libp2p network
    let libp2p_network = Arc::into_inner(
        create_libp2p_network(
            node_index,
            total_num_nodes,
            public_key,
            private_key,
            known_libp2p_nodes,
        )
        .await?,
    )
    .unwrap();

    // Create and return the combined network
    Ok(Arc::new(CombinedNetworks::new(
        cdn_network,
        libp2p_network,
        Some(Duration::from_secs(1)),
    )))
}

/// A macro to start a node with a given network. We need this because `NodeImplementation` requires we
/// define a bunch of traits, which we can't do in a generic context.
macro_rules! start_node_with_network {
    ($network_type:ident, $node_index:expr, $total_num_nodes:expr, $public_key:expr, $private_key:expr, $config:expr, $memberships:expr, $hotshot_initializer:expr, $network:expr, $join_set:expr, $num_transactions_per_view:expr, $transaction_size:expr, $num_views:expr, $builder_url:expr) => {
        // Create the marketplace config
        let marketplace_config = MarketplaceConfig::<_, $network_type> {
            auction_results_provider: TestAuctionResultsProvider::<TestTypes>::default().into(),
            // TODO: we need to pass a valid fallback builder url here somehow
            fallback_builder_url: Url::parse("http://localhost:8080").unwrap(),
        };

        // Initialize the system context
        let handle = SystemContext::<TestTypes, $network_type, TestVersions>::init(
            $public_key.clone(),
            $private_key.clone(),
            $config.clone(),
            $memberships.clone(),
            $network,
            $hotshot_initializer,
            ConsensusMetricsValue::default(),
            TestStorage::<TestTypes>::default(),
            marketplace_config,
        )
        .await
        .with_context(|| "Failed to initialize system context")?
        .0;

    // If the node index is 0, create and start the builder, which sips off events from HotShot
    if $node_index == 0 {
        // Create it
        let builder_handle =
            <SimpleBuilderImplementation as TestBuilderImplementation<TestTypes>>::start(
                $total_num_nodes,
                $builder_url.clone(),
                (),
            HashMap::new(),
        )
        .await;

        // Start it
        builder_handle.start(Box::new(handle.event_stream()));
    }

    // Start consensus
    handle.hotshot.start_consensus().await;

    // Create an LRU cache to store the size of the proposed block if we're DA
    let mut proposed_block_size_cache = LruCache::new(NonZero::new(100).unwrap());

    // Spawn the task into the join set
    $join_set.spawn(async move {
        // Get the event stream for this particular node
        let mut event_stream = handle.event_stream();

        // Wait for a `Decide` event for the view number we requested
        loop {
            // Get the next event
            let event = event_stream.next().await.unwrap();

            // DA proposals contain the full list of transactions. We can use this to cache
            // the size of the proposed block
            if let EventType::DaProposal { proposal, .. } = event.event {
                // Insert the size of the proposed block into the cache
                proposed_block_size_cache.put(
                    *proposal.data.view_number,
                    proposal.data.encoded_transactions.len(),
                );

                // A `Decide` event contains data that HotShot has decided on
            } else if let EventType::Decide { qc, .. } = event.event {
                // If we cached the size of the proposed block, log it
                if let Some(size) = proposed_block_size_cache.get(&*qc.view_number) {
                    info!(
                        "Decided on view number {}. Block size was {}",
                        qc.view_number, size
                    );
                } else {
                    info!("Decided on view number {}.", qc.view_number);
                }

                // If the view number is divisible by the node's index, submit transactions
                if $node_index == 0 {
                    // Generate and submit the requested number of transactions
                    for _ in 0..$num_transactions_per_view {
                        // Generate a random transaction
                        let mut transaction_bytes = vec![0u8; $transaction_size];
                        rand::thread_rng().fill(&mut transaction_bytes[..]);

                        // Submit the transaction
                        if let Err(err) = handle
                            .submit_transaction(TestTransaction::new(transaction_bytes))
                            .await
                        {
                            error!("Failed to submit transaction: {:?}", err);
                        };
                    }
                }

                // If we have a specific view number we want to wait for, check if we've reached it
                if let Some(num_views) = $num_views {
                    if *qc.view_number == num_views as u64 {
                        // Break when we've decided on the view number we requested
                        break;
                    }
                }
            }
            }
        });
    };
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> Result<()> {
    // Initialize logging
    initialize_logging();

    // Parse the command line arguments
    let args = Args::parse();

    // Match the network type
    let network_type = match args.network.to_lowercase().as_str() {
        "combined" => NetworkType::Combined,
        "cdn" => NetworkType::Cdn,
        "libp2p" => NetworkType::LibP2P,
        _ => {
            anyhow::bail!("Invalid network type. Please use one of 'combined', 'cdn', or 'libp2p'.")
        }
    };

    // Generate the builder URL we plan to use
    let builder_url = Url::parse(
        format!(
            "http://localhost:{}",
            portpicker::pick_unused_port().with_context(|| "Failed to find unused port")?
        )
        .as_str(),
    )
    .with_context(|| "Failed to parse builder URL")?;

    // Create the `known_nodes` and `known_da_nodes`
    let known_nodes: Vec<PeerConfig<BLSPubKey>> = (0..args.total_num_nodes)
        .map(peer_info_from_index)
        .collect();
    let known_da_nodes: Vec<PeerConfig<BLSPubKey>> = (0..args.num_da_nodes)
        .filter(|index| is_da_node(*index, args.num_da_nodes))
        .map(peer_info_from_index)
        .collect();

    // If the network type is "Libp2p" or "Combined", we need to also assign the list of
    // Libp2p addresses to be used
    let mut known_libp2p_nodes = Vec::new();
    if network_type == NetworkType::LibP2P || network_type == NetworkType::Combined {
        for index in 0..args.total_num_nodes {
            // Generate a Libp2p multiaddress from a random, unused port
            let addr = libp2p_multiaddress_from_index(index)
                .with_context(|| "Failed to generate multiaddress")?;
            known_libp2p_nodes.push(addr);
        }
    }

    // Create the memberships from the known nodes and known da nodes
    let memberships = StaticCommittee::new(known_nodes.clone(), known_da_nodes.clone());

    // Create a `JoinSet` composed of all handles
    let mut join_set = JoinSet::new();

    // Spawn each node
    for index in 0..args.total_num_nodes {
        // Create a new instance state
        let instance_state = TestInstanceState::new(DelayConfig::default());

        // Initialize HotShot from genesis
        let hotshot_initializer =
            hotshot::HotShotInitializer::<TestTypes>::from_genesis::<TestVersions>(instance_state)
                .await
                .with_context(|| "Failed to initialize HotShot")?;

        // Create our own keypair
        let (public_key, private_key) =
            BLSPubKey::generated_from_seed_indexed([0u8; 32], index as u64);

        // Configure HotShot
        let config = HotShotConfig::<BLSPubKey> {
            known_nodes: known_nodes.clone(),
            known_da_nodes: known_da_nodes.clone(),
            next_view_timeout: 5000,
            fixed_leader_for_gpuvid: 0, // This means that we don't have a fixed leader for testing GPU VID
            view_sync_timeout: Duration::from_secs(5),
            builder_timeout: Duration::from_secs(1),
            data_request_delay: Duration::from_millis(200),
            builder_urls: vec![builder_url.clone()],
            start_proposing_view: u64::MAX, // These just mean the upgrade functionality is disabled
            stop_proposing_view: u64::MAX,
            start_voting_view: u64::MAX,
            stop_voting_view: u64::MAX,
            start_proposing_time: u64::MAX,
            stop_proposing_time: u64::MAX,
            start_voting_time: u64::MAX,
            stop_voting_time: u64::MAX,
            epoch_height: 0, // This just means epochs aren't enabled
        };

        // Create a network and start HotShot based on the network type
        match network_type {
            NetworkType::Combined => {
                // Create the combined network
                let network = create_combined_network(
                    index,
                    args.total_num_nodes,
                    &known_libp2p_nodes,
                    is_da_node(index, args.num_da_nodes),
                    &public_key,
                    &private_key,
                )
                .await
                .with_context(|| "Failed to create Combined network")?;

                // Start the node
                start_node_with_network!(
                    CombinedImpl,
                    index,
                    args.total_num_nodes,
                    &public_key,
                    &private_key,
                    &config,
                    memberships,
                    hotshot_initializer,
                    network,
                    join_set,
                    args.num_transactions_per_view,
                    args.transaction_size,
                    args.num_views,
                    builder_url
                );
            }
            NetworkType::Cdn => {
                // Create the CDN network
                let network = create_cdn_network(
                    is_da_node(index, args.num_da_nodes),
                    &public_key,
                    &private_key,
                )
                .await
                .with_context(|| "Failed to create CDN network")?;

                // Start the node
                start_node_with_network!(
                    PushCdnImpl,
                    index,
                    args.total_num_nodes,
                    &public_key,
                    &private_key,
                    &config,
                    memberships,
                    hotshot_initializer,
                    network,
                    join_set,
                    args.num_transactions_per_view,
                    args.transaction_size,
                    args.num_views,
                    builder_url
                );
            }

            NetworkType::LibP2P => {
                // Create the Libp2p network
                let network = create_libp2p_network(
                    index,
                    args.total_num_nodes,
                    &public_key,
                    &private_key,
                    &known_libp2p_nodes,
                )
                .await
                .with_context(|| "Failed to create Libp2p network")?;

                // Start the node
                start_node_with_network!(
                    Libp2pImpl,
                    index,
                    args.total_num_nodes,
                    &public_key,
                    &private_key,
                    &config,
                    memberships,
                    hotshot_initializer,
                    network,
                    join_set,
                    args.num_transactions_per_view,
                    args.transaction_size,
                    args.num_views,
                    builder_url
                );
            }
        };
    }

    // Wait for all the tasks to finish
    while let Some(res) = join_set.join_next().await {
        res.expect("Failed to join task");
    }

    Ok(())
}
