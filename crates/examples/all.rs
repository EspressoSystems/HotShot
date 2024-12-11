//! This file contains an example of running a full HotShot network, comprised of
//! a CDN, Libp2p, a builder, and multiple validators.
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
            derive_libp2p_keypair, CdnMetricsValue, CdnTopic, KeyPair, Libp2pMetricsValue,
            Libp2pNetwork, PushCdnNetwork, TestingDef, WrappedSignatureKey,
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
use tokio::{spawn, sync::OnceCell};
use tracing::info;
use url::Url;

// Include some common code
include!("common.rs");

/// This example runs all necessary HotShot components
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

    /// The number of transactions to submit to each nodes' builder per view
    #[arg(long, default_value_t = 1)]
    num_transactions_per_view: usize,

    /// The size of the transactions submitted to each nodes' builder per view
    #[arg(long, default_value_t = 1000)]
    transaction_size: usize,

    /// The type of network to use. Acceptable values are
    /// "combined", "cdn", or "libp2p"
    #[arg(long, default_value = "combined")]
    network: String,
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

/// A helper function to start the CDN
/// (2 brokers + 1 marshal)
///
/// Returns the address of the marshal
async fn start_cdn() -> Result<String> {
    // Figure out where we're going to spawn the marshal
    let marshal_port =
        portpicker::pick_unused_port().with_context(|| "Failed to find unused port")?;
    let marshal_address = format!("127.0.0.1:{marshal_port}");

    // Generate a random file path for the SQLite database
    let db_path = format!("/tmp/marshal-{}.db", rand::random::<u32>());

    // Configure the marshal
    let marshal_config = MarshalConfig {
        bind_endpoint: marshal_address.clone(),
        discovery_endpoint: db_path.clone(),
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
                discovery_endpoint: db_path.clone(),
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

/// Start the CDN if it's not already running
/// Returns the address of the marshal
async fn try_start_cdn() -> Result<String> {
    /// Create a static cell to store the marshal's endpoint
    static MARSHAL_ADDRESS: OnceCell<String> = OnceCell::const_new();

    // If the marshal endpoint isn't already set, start the CDN and set the endpoint
    Ok(MARSHAL_ADDRESS
        .get_or_init(|| async { start_cdn().await.expect("Failed to start CDN") })
        .await
        .clone())
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

    // Create a set composed of all handles
    let mut join_set = Vec::new();

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
                // Start the CDN if it's not already running
                let marshal_address = try_start_cdn().await?;

                // Create the combined network
                let network = new_combined_network(
                    Some(marshal_address),
                    known_libp2p_nodes[index].clone(),
                    args.total_num_nodes,
                    &known_libp2p_nodes,
                    is_da_node(index, args.num_da_nodes),
                    &public_key,
                    &private_key,
                )
                .await
                .with_context(|| "Failed to create Combined network")?;

                // Start the node
                join_set.push(
                    start_consensus::<CombinedImpl>(
                        public_key,
                        private_key,
                        config,
                        memberships.clone(),
                        network,
                        hotshot_initializer,
                        args.total_num_nodes,
                        builder_url.clone(),
                        args.num_transactions_per_view,
                        args.transaction_size,
                        args.num_views,
                    )
                    .await?,
                );
            }
            NetworkType::Cdn => {
                // Start the CDN if it's not already running
                let marshal_address = try_start_cdn().await?;

                // Create the CDN network
                let network = new_cdn_network(
                    Some(marshal_address),
                    is_da_node(index, args.num_da_nodes),
                    &public_key,
                    &private_key,
                )
                .with_context(|| "Failed to create CDN network")?;

                // Start the node
                join_set.push(
                    start_consensus::<PushCdnImpl>(
                        public_key,
                        private_key,
                        config,
                        memberships.clone(),
                        network,
                        hotshot_initializer,
                        args.total_num_nodes,
                        builder_url.clone(),
                        args.num_transactions_per_view,
                        args.transaction_size,
                        args.num_views,
                    )
                    .await?,
                );
            }

            NetworkType::LibP2P => {
                // Create the Libp2p network
                let network = new_libp2p_network(
                    // Advertise == bind address here
                    known_libp2p_nodes[index].clone(),
                    args.total_num_nodes,
                    &public_key,
                    &private_key,
                    &known_libp2p_nodes,
                )
                .await
                .with_context(|| "Failed to create Libp2p network")?;

                // Start the node
                join_set.push(
                    start_consensus::<Libp2pImpl>(
                        public_key,
                        private_key,
                        config,
                        memberships.clone(),
                        network,
                        hotshot_initializer,
                        args.total_num_nodes,
                        builder_url.clone(),
                        args.num_transactions_per_view,
                        args.transaction_size,
                        args.num_views,
                    )
                    .await?,
                );
            }
        };
    }

    // Wait for all the tasks to finish
    while let Some(res) = join_set.pop() {
        res.await.expect("Failed to join task");
    }

    Ok(())
}
