use std::time::Instant;

use hotshot_types::{
    data::EpochNumber, traits::node_implementation::ConsensusTime, utils::non_crypto_hash,
};
use simple_moving_average::SingleSumSMA;
use simple_moving_average::SMA;

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

/// This is a testing function which allows us to easily determine if a node should be a DA node
fn is_da_node(index: usize, num_da_nodes: usize) -> bool {
    index < num_da_nodes
}

/// This is a testing function which allows us to easily generate peer configs from indexes
fn peer_info_from_index(index: usize) -> hotshot_types::PeerConfig<hotshot::types::BLSPubKey> {
    // Get the node's public key
    let (public_key, _) =
        hotshot::types::BLSPubKey::generated_from_seed_indexed([0u8; 32], index as u64);

    // Generate the peer config
    hotshot_types::PeerConfig {
        stake_table_entry: public_key.stake_table_entry(1),
        state_ver_key: hotshot_types::light_client::StateKeyPair::default()
            .0
            .ver_key(),
    }
}

/// Create a new Push CDN network
fn new_cdn_network(
    marshal_address: Option<String>,
    is_da_node: bool,
    public_key: &BLSPubKey,
    private_key: &BLSPrivKey,
) -> Result<Arc<PushCdnNetwork<BLSPubKey>>> {
    // If the marshal endpoint is not provided, we don't need to create a CDN network
    let Some(marshal_address) = marshal_address else {
        anyhow::bail!("Marshal endpoint is required for CDN networks");
    };

    // Subscribe to topics based on whether we're a DA node or not
    let mut topics = vec![CdnTopic::Global];
    if is_da_node {
        topics.push(CdnTopic::Da);
    }

    // Create and return the network
    Ok(Arc::new(
        PushCdnNetwork::new(
            marshal_address,
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

/// A helper function to create a Libp2p network
async fn new_libp2p_network(
    bind_address: Multiaddr,
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
        bind_address,
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

/// A helper function to create a Combined network, which is a combination of a Libp2p and Push CDN network
async fn new_combined_network(
    marshal_address: Option<String>,
    libp2p_bind_address: Multiaddr,
    total_num_nodes: usize,
    known_libp2p_nodes: &[Multiaddr],
    is_da_node: bool,
    public_key: &BLSPubKey,
    private_key: &BLSPrivKey,
) -> Result<Arc<hotshot::traits::implementations::CombinedNetworks<TestTypes>>> {
    // Create the CDN network and launch the CDN
    let cdn_network = Arc::into_inner(new_cdn_network(
        marshal_address,
        is_da_node,
        public_key,
        private_key,
    )?)
    .unwrap();

    // Create the Libp2p network
    let libp2p_network = Arc::into_inner(
        new_libp2p_network(
            libp2p_bind_address,
            total_num_nodes,
            public_key,
            private_key,
            known_libp2p_nodes,
        )
        .await?,
    )
    .unwrap();

    // Create and return the combined network
    Ok(Arc::new(
        hotshot::traits::implementations::CombinedNetworks::new(
            cdn_network,
            libp2p_network,
            Some(Duration::from_secs(1)),
        ),
    ))
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::cast_sign_loss)]
#[allow(clippy::too_many_lines)]
#[allow(clippy::cast_possible_truncation)]
/// A helper function to start consensus with a builder
async fn start_consensus<
    I: hotshot::traits::NodeImplementation<
        TestTypes,
        Storage = TestStorage<TestTypes>,
        AuctionResultsProvider = TestAuctionResultsProvider<TestTypes>,
    >,
>(
    public_key: BLSPubKey,
    private_key: BLSPrivKey,
    config: HotShotConfig<BLSPubKey>,
    memberships: hotshot_example_types::node_types::StaticMembership,
    network: Arc<I::Network>,
    hotshot_initializer: hotshot::HotShotInitializer<TestTypes>,
    total_num_nodes: usize,
    builder_url: Url,
    num_transactions_per_view: usize,
    transaction_size: usize,
    num_views: Option<usize>,
) -> Result<tokio::task::JoinHandle<()>> {
    // Create the marketplace config
    let marketplace_config: MarketplaceConfig<TestTypes, I> = MarketplaceConfig {
        auction_results_provider: TestAuctionResultsProvider::<TestTypes>::default().into(),
        // TODO: we need to pass a valid fallback builder url here somehow
        fallback_builder_url: Url::parse("http://localhost:8080").unwrap(),
    };

    // Initialize the system context
    let handle = SystemContext::<TestTypes, I, TestVersions>::init(
        public_key,
        private_key,
        config,
        memberships.clone(),
        network,
        hotshot_initializer,
        ConsensusMetricsValue::default(),
        TestStorage::<TestTypes>::default(),
        marketplace_config,
    )
    .await
    .with_context(|| "Failed to initialize system context")?
    .0;

    // Each node has to start the builder since we don't have a sovereign builder in this example
    let builder_handle =
        <SimpleBuilderImplementation as TestBuilderImplementation<TestTypes>>::start(
            total_num_nodes,
            builder_url.clone(),
            (),
            HashMap::new(),
        )
        .await;

    // Start it
    builder_handle.start(Box::new(handle.event_stream()));

    // Start consensus
    handle.hotshot.start_consensus().await;

    // See if we're a DA node or not
    let is_da_node = memberships.has_da_stake(&public_key, EpochNumber::new(0));

    // Create an LRU cache to store block data if we're DA. We populate this cache when we receive
    // the DA proposal for a view and print the data when we actually decide on that view
    let mut view_cache = LruCache::new(NonZero::new(100).unwrap());

    // A cache of outstanding transactions (hashes of). Used to calculate latency. Isn't needed for non-DA nodes
    let mut outstanding_transactions: LruCache<u64, Instant> =
        LruCache::new(NonZero::new(10000).unwrap());

    // The simple moving average, used to calculate throughput
    let mut throughput: SingleSumSMA<f64, f64, 100> = SingleSumSMA::<f64, f64, 100>::new();

    // The last time we decided on a view (for calculating throughput)
    let mut last_decide_time = Instant::now();

    // Spawn the task to wait for events
    let join_handle = tokio::spawn(async move {
        // Get the event stream for this particular node
        let mut event_stream = handle.event_stream();

        // Wait for a `Decide` event for the view number we requested
        loop {
            // Get the next event
            let event = event_stream.next().await.unwrap();

            // DA proposals contain the full list of transactions. We can use this to cache
            // the size of the proposed block
            if let EventType::DaProposal { proposal, .. } = event.event {
                // Decode the transactions. We use this to log the size of the proposed block
                // when we decide on a view
                let transactions =
                    match TestTransaction::decode(&proposal.data.encoded_transactions) {
                        Ok(transactions) => transactions,
                        Err(err) => {
                            tracing::error!("Failed to decode transactions: {:?}", err);
                            continue;
                        }
                    };

                // Get the number of transactions in the proposed block
                let num_transactions = transactions.len();

                // Sum the total number of bytes in the proposed block and cache
                // the hash so we can calculate latency
                let mut sum = 0;
                let mut submitted_times = Vec::new();
                for transaction in transactions {
                    // Add the size of the transaction to the sum
                    sum += transaction.bytes().len();

                    // If we can find the transaction in the cache, add the hash of the transaction to the cache
                    if let Some(&instant) =
                        outstanding_transactions.get(&non_crypto_hash(transaction.bytes()))
                    {
                        submitted_times.push(instant);
                    }
                }

                // Insert the size of the proposed block and the number of transactions into the cache.
                // We use this to log the size of the proposed block when we decide on a view
                view_cache.put(
                    *proposal.data.view_number,
                    (sum, num_transactions, submitted_times),
                );

                // A `Decide` event contains data that HotShot has decided on
            } else if let EventType::Decide { qc, .. } = event.event {
                // If we cached the size of the proposed block, log it
                if let Some((block_size, num_transactions, submitted_times)) =
                    view_cache.get(&*qc.view_number)
                {
                    // Calculate the average latency of the transactions
                    let mut total_latency = Duration::default();
                    let mut num_found_transactions = 0;
                    for submitted_time in submitted_times {
                        total_latency += submitted_time.elapsed();
                        num_found_transactions += 1;
                    }
                    let average_latency = total_latency.checked_div(num_found_transactions);

                    // Update the throughput SMA
                    throughput
                        .add_sample(*block_size as f64 / last_decide_time.elapsed().as_secs_f64());

                    // Update the last decided time
                    last_decide_time = Instant::now();

                    // If we have a valid average latency, log it
                    if let Some(average_latency) = average_latency {
                        info!(
                        block_size = block_size,
                        num_txs = num_transactions,
                        avg_tx_latency =? average_latency,
                        avg_throughput = format!("{}/s", bytesize::ByteSize::b(throughput.get_average() as u64)),
                        "Decided on view {}",
                            *qc.view_number
                        );
                    } else {
                        info!(
                            block_size = block_size,
                            num_txs = num_transactions,
                            "Decided on view {}",
                            *qc.view_number
                        );
                    }
                } else {
                    info!("Decided on view {}", *qc.view_number);
                }

                // Generate and submit the requested number of transactions
                for _ in 0..num_transactions_per_view {
                    // Generate a random transaction
                    let mut transaction_bytes = vec![0u8; transaction_size];
                    rand::thread_rng().fill(&mut transaction_bytes[..]);

                    // If we're a DA node, cache the transaction so we can calculate latency
                    if is_da_node {
                        outstanding_transactions
                            .put(non_crypto_hash(&transaction_bytes), Instant::now());
                    }

                    // Submit the transaction
                    if let Err(err) = handle
                        .submit_transaction(TestTransaction::new(transaction_bytes))
                        .await
                    {
                        tracing::error!("Failed to submit transaction: {:?}", err);
                    };
                }

                // If we have a specific view number we want to wait for, check if we've reached it
                if let Some(num_views) = num_views {
                    if *qc.view_number == num_views as u64 {
                        // Break when we've decided on the view number we requested
                        break;
                    }
                }
            }
        }
    });

    Ok(join_handle)
}
