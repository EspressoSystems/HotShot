//! Types and Traits for the `HotShot` consensus module
#![warn(
    clippy::all,
    clippy::pedantic,
    rust_2018_idioms,
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::panic
)]
#![allow(clippy::module_name_repetitions)]

use displaydoc::Display;
use jf_primitives::signatures::{AggregateableSignatureSchemes, SignatureScheme};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use stake_table::StakeTableEntry;
use std::{num::NonZeroUsize, time::Duration};
use traits::election::ElectionConfig;
pub mod consensus;
pub mod data;
pub mod error;
pub mod event;
pub mod light_client;
pub mod message;
// pub mod signature_key;
pub mod simple_certificate;
pub mod simple_vote;
pub mod stake_table;
pub mod traits;
pub mod utils;
pub mod vote;

/// the type of consensus to run. Either:
/// wait for a signal to start a view,
/// or constantly run
/// you almost always want continuous
/// incremental is just for testing
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum ExecutionType {
    /// constantly increment view as soon as view finishes
    Continuous,
    /// wait for a signal
    Incremental,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Display)]
#[serde(bound(deserialize = ""))]
/// config for validator, including public key, private key, stake value
pub struct ValidatorConfig<A: AggregateableSignatureSchemes> {
    /// The validator's public key and stake value
    pub public_key: <A as SignatureScheme>::VerificationKey,
    /// The validator's private key, should be in the mempool, not public
    pub private_key: <A as SignatureScheme>::SigningKey,
    /// The validator's stake
    pub stake_value: u64,
    /// the validator's key pairs for state signing/verification
    pub state_key_pair: light_client::StateKeyPair,
}

impl<A: AggregateableSignatureSchemes> ValidatorConfig<A> {
    /// generate validator config from input seed, index and stake value
    #[must_use]
    pub fn generated_from_seed_indexed(seed: [u8; 32], index: u64, stake_value: u64) -> Self {
        // Getting new seed
        let mut hasher = blake3::Hasher::new();
        hasher.update(&seed);
        hasher.update(&index.to_le_bytes());
        let new_seed = *hasher.finalize().as_bytes();

        let pp = <A as SignatureScheme>::param_gen::<ChaCha20Rng>(None)
            .expect("Signature public parameter generations shouldn't fail.");
        let (private_key, public_key) =
            <A as SignatureScheme>::key_gen(&pp, &mut ChaCha20Rng::from_seed(seed))
                .expect("Signature key pairs generation shouldn't fail.");
        let state_key_pairs =
            light_client::StateKeyPair::generate(&mut ChaCha20Rng::from_seed(seed));
        Self {
            public_key,
            private_key,
            stake_value,
            state_key_pair: state_key_pairs,
        }
    }
}

impl<A: AggregateableSignatureSchemes> Default for ValidatorConfig<A> {
    fn default() -> Self {
        Self::generated_from_seed_indexed([0u8; 32], 0, 1)
    }
}

/// Holds configuration for a `HotShot`
#[derive(Clone, custom_debug::Debug, serde::Serialize, serde::Deserialize)]
#[serde(bound(deserialize = ""))]
pub struct HotShotConfig<A: AggregateableSignatureSchemes, ELECTIONCONFIG: ElectionConfig> {
    /// Whether to run one view or continuous views
    pub execution_type: ExecutionType,
    /// Total number of nodes in the network
    pub total_nodes: NonZeroUsize,
    /// Minimum transactions per block
    pub min_transactions: usize,
    /// Maximum transactions per block
    pub max_transactions: NonZeroUsize,
    /// List of known node's public keys and stake value for certificate aggregation, serving as public parameter
    pub known_nodes_with_stake: Vec<StakeTableEntry<<A as SignatureScheme>::VerificationKey>>,
    /// My own validator config, including my public key, private key, stake value, serving as private parameter
    pub my_own_validator_config: ValidatorConfig<A>,
    /// List of DA committee nodes for static DA committe
    pub da_committee_size: usize,
    /// Base duration for next-view timeout, in milliseconds
    pub next_view_timeout: u64,
    /// The exponential backoff ration for the next-view timeout
    pub timeout_ratio: (u64, u64),
    /// The delay a leader inserts before starting pre-commit, in milliseconds
    pub round_start_delay: u64,
    /// Delay after init before starting consensus, in milliseconds
    pub start_delay: u64,
    /// Number of network bootstrap nodes
    pub num_bootstrap: usize,
    /// The minimum amount of time a leader has to wait to start a round
    pub propose_min_round_time: Duration,
    /// The maximum amount of time a leader can wait to start a round
    pub propose_max_round_time: Duration,
    /// the election configuration
    pub election_config: Option<ELECTIONCONFIG>,
}
