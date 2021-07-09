use phaselock::{
    demos::dentry::*,
    event::{Event, EventType},
    handle::PhaseLockHandle,
    message::Message,
    networking::w_network::WNetwork,
    tc, PhaseLock, PhaseLockConfig, PubKey, H_256,
};
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use structopt::StructOpt;
use toml::Value;
use tracing::{debug, error, instrument};

/// Holds configuration for a seed that will be used to generate a secret key.
#[derive(Clone, Debug)]
pub struct SeedConfig {
    /// Machine ID
    // TODO: is this field useless?
    pub machine_id: u64,
    /// Seed to generate a secret key
    pub seed: u64,
}

#[derive(Debug, StructOpt)]
#[structopt(
    name = "Multi-machine concensus",
    about = "Simulates consensus among multiple machines"
)]
struct Opt {
    /// Id of the current node
    #[structopt(short = "i", default_value = "1")]
    id: u64,
}

/// Reads nodes from node configuration file.
fn read_nodes_from_config<R: rand::Rng>(node_config_path: &Path, mut rng: &mut R) {
    // Read node info from node configuration file
    let mut node_file = match File::open(&node_config_path) {
        Ok(file) => file,
        Err(_) => {
            panic!("Cannot find node config file");
        }
    };
    let mut toml_content = String::new();
    node_file
        .read_to_string(&mut toml_content)
        .unwrap_or_else(|err| panic!("Error while reading node config: [{}]", err));

    let node_info: Value = match toml::from_str(&toml_content) {
        Ok(info) => info,
        Err(_) => {
            panic!("Error while reading node config file")
        }
    };

    // Get the seed and node count and use it to generate secret key set
    let seed: u64 = match node_info["seed"].as_integer() {
        Some(seed) => seed.unsigned_abs(),
        None => {
            panic!("Missing seed value")
        }
    };
    let nodes = match node_info["node_count"].as_integer() {
        Some(nodes) => nodes,
        None => {
            panic!("Missing node count")
        }
    };
    let threshold = ((nodes * 2) / 3) + 1;
    let mut rng = Xoshiro256StarStar::seed_from_u64(seed);
    let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut rng);

    // Get the node ID from CL
    let opt = Opt::from_args();
    debug!(?opt);
    let own_key = phaselock::PubKey::from_secret_key_set_escape_hatch(&sks, opt.id);
}

#[async_std::main]
#[instrument]
async fn main() {}
