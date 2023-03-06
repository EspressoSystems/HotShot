#![allow(unused)] 

use std::{
    cmp,
    collections::{BTreeSet, VecDeque},
    fs, mem,
    net::{IpAddr, SocketAddr},
    num::NonZeroUsize,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use async_std::task::sleep;
use surf_disco::{error::ClientError, Error};

use async_compatibility_layer::{
    art::{async_sleep, TcpStream},
    logging::{setup_backtrace, setup_logging},
};
use async_lock::RwLock;
use async_trait::async_trait;
use clap::Parser;
use hotshot::{
    demos::vdemo::VDemoTypes,
    traits::{
        implementations::{
            CentralizedWebCommChannel, CentralizedWebServerNetwork, Libp2pCommChannel, Libp2pNetwork,
            MemoryStorage,
        },
        NetworkError, NodeImplementation, Storage,
    },
    types::{HotShotHandle, SignatureKey},
    HotShot, ViewRunner,
};
use hotshot_types::{
    data::{TestableLeaf, ValidatingLeaf, ValidatingProposal},
    traits::{
        election::Membership,
        metrics::NoMetrics,
        network::CommunicationChannel,
        node_implementation::NodeType,
        state::{TestableBlock, TestableState},
    },
    vote::QuorumVote,
    HotShotConfig,
};
use hotshot_orchestrator::{
    self,
    // TODO ED Rename this once we get rid of old file
    config::{NetworkConfig as OtherNetworkConfig, NetworkConfigFile as OtherNetworkConfigFile},
};
use tracing::{debug, error};



#[derive(Parser, Debug, Clone)]
#[command(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]

pub struct OrchestratorArgs {
    /// The address the orchestrator runs on
    host: IpAddr,
    /// The port the orchestrator runs on
    port: u16,
    /// The configuration file to be used for this run
    config_file: String,
}

// TODO ED Does this need to actually return a result? Doesn't seem like it
// This only reads one file, unlike the old server that read multiple.  We didn't use that featuree anyway
pub fn load_config_from_file<TYPES: NodeType>(
    config_file: String,
) -> OtherNetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType> {
    let config_file_as_string: String = fs::read_to_string(config_file.as_str())
        .expect(format!("Could not read config file located at {config_file}").as_str());
    let config_toml: OtherNetworkConfigFile =
        toml::from_str::<OtherNetworkConfigFile>(&config_file_as_string)
            .expect("Unable to convert config file to TOML");

    let mut config: OtherNetworkConfig<TYPES::SignatureKey, TYPES::ElectionConfigType> =
        config_toml.into();

    // Generate keys
    config.config.known_nodes = (0..config.config.total_nodes.get())
        .map(|node_id| {
            TYPES::SignatureKey::generated_from_seed_indexed(
                config.seed,
                node_id.try_into().unwrap(),
            )
            .0
        })
        .collect();
    config
}

pub async fn run_orchestrator<
    TYPES: NodeType,
    MEMBERSHIP: Membership<TYPES>,
    NETWORK: CommunicationChannel<
        TYPES,
        ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    >,
    NODE: NodeImplementation<
        TYPES,
        Leaf = ValidatingLeaf<TYPES>,
        Proposal = ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        Membership = MEMBERSHIP,
        Networking = NETWORK,
        Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
    >,
>(OrchestratorArgs{
    host,
    port,
    config_file,
}: OrchestratorArgs) {
    println!("Starting orchestrator",);
    let run_config = load_config_from_file::<TYPES>(config_file);

    println!("{:?}", run_config);
    let _result =
        hotshot_orchestrator::run_orchestrator::<TYPES::SignatureKey, TYPES::ElectionConfigType>(run_config, host, port).await;



}

// VALIDATOR

#[derive(Parser, Debug, Clone)]
#[command(
    name = "Multi-machine consensus",
    about = "Simulates consensus among multiple machines"
)]

pub struct ValidatorArgs {
    /// The address the orchestrator runs on
    host: IpAddr,
    /// The port the orchestrator runs on
    port: u16,
}

pub async fn main_entry_point<
    TYPES: NodeType,
    MEMBERSHIP: Membership<TYPES>,
    NETWORK: CommunicationChannel<
        TYPES,
        ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        QuorumVote<TYPES, ValidatingLeaf<TYPES>>,
        MEMBERSHIP,
    >,
    NODE: NodeImplementation<
        TYPES,
        Leaf = ValidatingLeaf<TYPES>,
        Proposal = ValidatingProposal<TYPES, ValidatingLeaf<TYPES>>,
        Membership = MEMBERSHIP,
        Networking = NETWORK,
        Storage = MemoryStorage<TYPES, ValidatingLeaf<TYPES>>,
    >,
>(
    args: ValidatorArgs,
) where
    <TYPES as NodeType>::StateType: TestableState,
    <TYPES as NodeType>::BlockType: TestableBlock,
    ValidatingLeaf<TYPES>: TestableLeaf,
    HotShot<TYPES::ConsensusType, TYPES, NODE>: ViewRunner<TYPES, NODE>,
{
    setup_logging();
    setup_backtrace();

    print!("Running validator");
}
