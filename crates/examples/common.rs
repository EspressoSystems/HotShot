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
