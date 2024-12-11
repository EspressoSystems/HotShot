//! This is meant to be run externally, e.g. when running benchmarks on the protocol.
//! If you just want to run everything required, you can use the `all` example
//!
//! This example runs a single validator node along with a simple builder (since the
//! real builder is in an upstream repo)

use std::{
    collections::HashMap, net::IpAddr, num::NonZero, str::FromStr, sync::Arc, time::Duration,
};

use anyhow::{Context, Result};
use clap::Parser;
use futures::StreamExt;
use hotshot::{
    helpers::initialize_logging,
    traits::{
        election::static_committee::StaticCommittee,
        implementations::{
            derive_libp2p_keypair, CdnMetricsValue, CdnTopic, KeyPair, Libp2pMetricsValue,
            Libp2pNetwork, PushCdnNetwork, WrappedSignatureKey,
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
    consensus::ConsensusMetricsValue, traits::election::Membership, HotShotConfig,
};
use libp2p::Multiaddr;
use libp2p_networking::network::{
    behaviours::dht::record::{Namespace, RecordKey, RecordValue},
    node::config::{KademliaConfig, Libp2pConfig},
    GossipConfig, RequestResponseConfig,
};
use lru::LruCache;
use rand::Rng;
use tracing::info;
use url::Url;

// Include some common code
include!("common.rs");

/// This example runs a single validator node
#[derive(Parser)]
struct Args {
    /// The coordinator address to connect to. The coordinator is just used to tell other nodes
    /// about the libp2p bootstrap addresses and give each one a unique index
    #[arg(long, default_value = "http://127.0.0.1:3030")]
    coordinator_address: String,

    /// The marshal's endpoint to use. Only required if the network type is "cdn" or "combined"
    #[arg(long)]
    marshal_address: Option<String>,

    /// The source of the public IP address to use. This is used to generate the libp2p
    /// bootstrap addresses. Acceptable values are "ipify", "local", "localhost", "aws-local", or
    /// "aws-public"
    #[arg(long, default_value = "localhost")]
    ip_source: String,

    /// The port to use for Libp2p
    #[arg(long, default_value_t = 3000)]
    libp2p_port: u16,

    /// The number of nodes in the network. This needs to be the same between all nodes
    #[arg(long, default_value_t = 5)]
    total_num_nodes: usize,

    /// The number of nodes which are DA. This needs to be the same between all nodes
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

/// Get the IP address to use based on the source
async fn get_public_ip_address(source: &str) -> Result<IpAddr> {
    // Get the IP to use based on the source
    Ok(match source.to_lowercase().as_str() {
        "ipify" => reqwest::get("https://api.ipify.org")
            .await?
            .text()
            .await?
            .parse::<IpAddr>()
            .with_context(|| "Failed to parse IP address from IPify")?,
        "local" => {
            local_ip_address::local_ip().with_context(|| "Failed to get local IP address")?
        }
        "localhost" => "127.0.0.1".parse::<IpAddr>().unwrap(),
        "aws-local" => reqwest::get("http://169.254.169.254/latest/meta-data/local-ipv4")
            .await?
            .text()
            .await?
            .parse::<IpAddr>()
            .with_context(|| "Failed to parse IP address from AWS local")?,
        "aws-public" => reqwest::get("http://169.254.169.254/latest/meta-data/public-ipv4")
            .await?
            .text()
            .await?
            .parse::<IpAddr>()
            .with_context(|| "Failed to parse IP address from AWS public")?,
        _ => {
            anyhow::bail!(
                "Invalid public IP source. Please use one of 'ipify', 'local', 'localhost', 'aws-local', or 'aws-public'."
            )
        }
    })
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> Result<()> {
    // Initialize logging
    initialize_logging();

    // Parse the command-line arguments
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

    // Get the IP address to use for Libp2p based on the source
    let libp2p_ip = get_public_ip_address(&args.ip_source).await?;
    info!("Using Libp2p address: {}:{}", libp2p_ip, args.libp2p_port);

    // Create a new instance state
    let instance_state = TestInstanceState::new(DelayConfig::default());

    // Initialize HotShot from genesis
    let hotshot_initializer =
        hotshot::HotShotInitializer::<TestTypes>::from_genesis::<TestVersions>(instance_state)
            .await
            .with_context(|| "Failed to initialize HotShot")?;

    // Get our index from the coordinator
    let index = reqwest::get(format!("{}/index", &args.coordinator_address).as_str())
        .await?
        .text()
        .await?
        .parse::<usize>()
        .with_context(|| "Failed to parse index from coordinator")?;

    // Derive our keypair from the index we got
    let (public_key, private_key) = BLSPubKey::generated_from_seed_indexed([0u8; 32], index as u64);

    // Derive our libp2p keypair from the private key
    let peer_libp2p_keypair = derive_libp2p_keypair::<BLSPubKey>(&private_key)
        .with_context(|| "Failed to derive libp2p keypair")?;

    // Generate our advertise Multiaddr
    let advertise_multiaddr = format!(
        "/ip4/{}/udp/{}/quic-v1/p2p/{}",
        libp2p_ip,
        args.libp2p_port,
        peer_libp2p_keypair.public().to_peer_id()
    );

    // Generate our bind Multiaddr
    let bind_multiaddr =
        Multiaddr::from_str(&format!("/ip4/0.0.0.0/udp/{}/quic-v1", args.libp2p_port))
            .with_context(|| "Failed to parse bind Multiaddr")?;

    // Post our advertise libp2p address to the coordinator
    reqwest::Client::new()
        .post(format!("{}/libp2p-info", &args.coordinator_address).as_str())
        .body(advertise_multiaddr)
        .send()
        .await?;

    // Get the other libp2p addresses from the coordinator
    let known_libp2p_nodes =
        reqwest::get(format!("{}/libp2p-info", &args.coordinator_address).as_str())
            .await?
            .text()
            .await?
            .split('\n')
            .map(|s| {
                s.parse::<Multiaddr>()
                    .with_context(|| "Failed to parse Libp2p bootstrap address")
            })
            .collect::<Result<Vec<_>>>()?;

    // Print the known libp2p nodes
    info!("Known libp2p nodes: {:?}", known_libp2p_nodes);

    // Generate the builder URL we plan to use
    let builder_url = Url::parse(
        format!(
            "http://localhost:{}",
            portpicker::pick_unused_port().with_context(|| "Failed to find unused port")?
        )
        .as_str(),
    )
    .with_context(|| "Failed to parse builder URL")?;

    // Create the known nodes up to the total number of nodes
    let known_nodes: Vec<_> = (0..args.total_num_nodes)
        .map(peer_info_from_index)
        .collect();
    let known_da_nodes: Vec<_> = (0..args.num_da_nodes)
        .filter(|i| is_da_node(*i, args.num_da_nodes))
        .map(peer_info_from_index)
        .collect();

    // Create the memberships from the known nodes and known da nodes
    let memberships = StaticCommittee::new(known_nodes.clone(), known_da_nodes.clone());

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

    // Create the network and start consensus
    match network_type {
        NetworkType::Cdn => {
            // Create the network
            let network = new_cdn_network(
                args.marshal_address,
                is_da_node(index, args.num_da_nodes),
                &public_key,
                &private_key,
            )
            .with_context(|| "Failed to create CDN network")?;

            // Start consensus
            let join_handle = start_consensus::<PushCdnImpl>(
                public_key,
                private_key,
                config,
                memberships,
                network,
                hotshot_initializer,
                args.total_num_nodes,
                builder_url,
                args.num_transactions_per_view,
                args.transaction_size,
                args.num_views,
            )
            .await?;

            // Wait for consensus to finish
            join_handle.await?;
        }
        NetworkType::LibP2P => {
            // Create the network
            let network = new_libp2p_network(
                bind_multiaddr,
                args.total_num_nodes,
                &public_key,
                &private_key,
                &known_libp2p_nodes,
            )
            .await
            .with_context(|| "Failed to create libp2p network")?;

            // Start consensus
            let join_handle = start_consensus::<Libp2pImpl>(
                public_key,
                private_key,
                config,
                memberships,
                network,
                hotshot_initializer,
                args.total_num_nodes,
                builder_url,
                args.num_transactions_per_view,
                args.transaction_size,
                args.num_views,
            )
            .await?;

            // Wait for consensus to finish
            join_handle.await?;
        }
        NetworkType::Combined => {
            // Create the network
            let network = new_combined_network(
                args.marshal_address,
                bind_multiaddr,
                args.total_num_nodes,
                &known_libp2p_nodes,
                is_da_node(index, args.num_da_nodes),
                &public_key,
                &private_key,
            )
            .await
            .with_context(|| "Failed to create combined network")?;

            // Start consensus
            let join_handle = start_consensus::<CombinedImpl>(
                public_key,
                private_key,
                config,
                memberships,
                network,
                hotshot_initializer,
                args.total_num_nodes,
                builder_url,
                args.num_transactions_per_view,
                args.transaction_size,
                args.num_views,
            )
            .await?;

            // Wait for consensus to finish
            join_handle.await?;
        }
    };

    Ok(())
}
