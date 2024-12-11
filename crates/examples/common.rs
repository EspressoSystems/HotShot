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
    let cdn_network = Arc::into_inner(
        new_cdn_network(marshal_address, is_da_node, public_key, private_key)?,
    )
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
        memberships,
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

    // Create an LRU cache to store the size of the proposed block if we're DA
    let mut proposed_block_size_cache = LruCache::new(NonZero::new(100).unwrap());

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
                // Insert the size of the proposed block into the cache
                proposed_block_size_cache.put(
                    *proposal.data.view_number,
                    proposal.data.encoded_transactions.len(),
                );

                // A `Decide` event contains data that HotShot has decided on
            } else if let EventType::Decide { qc, .. } = event.event {
                // If we cached the size of the proposed block, log it
                if let Some(size) = proposed_block_size_cache.get(&*qc.view_number) {
                    info!(block_size = size, "Decided on view {}", *qc.view_number);
                } else {
                    info!("Decided on view {}", *qc.view_number);
                }

                // Generate and submit the requested number of transactions
                for _ in 0..num_transactions_per_view {
                    // Generate a random transaction
                    let mut transaction_bytes = vec![0u8; transaction_size];
                    rand::thread_rng().fill(&mut transaction_bytes[..]);

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
