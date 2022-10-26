use crate::{ClientConfig, NetworkConfig, Run, RunResults, ToBackground};
use hotshot_utils::art::{async_sleep, async_spawn};
use hotshot_utils::channel::{OneShotSender, Sender};
use libp2p_core::PeerId;
use std::{
    fmt::Debug,
    fs,
    net::{IpAddr, SocketAddr},
    path::Path,
    time::Duration,
};
use tracing::error;

/// Contains information about the current round
pub struct RoundConfig<K, E> {
    configs: Vec<NetworkConfig<K, E>>,
    libp2p_config_sender: Vec<(IpAddr, OneShotSender<ClientConfig<K, E>>)>,
    current_run: usize,
    next_node_index: usize,
}

impl<K, E> RoundConfig<K, E> {
    pub fn new(configs: Vec<NetworkConfig<K, E>>) -> Self {
        Self {
            configs,
            libp2p_config_sender: Vec::new(),
            current_run: 0,
            next_node_index: 0,
        }
    }

    pub async fn current_round_client_count(&self) -> usize {
        self.configs
            .get(self.current_run)
            .map(|r| r.config.total_nodes.get())
            .unwrap_or(0)
    }

    /// Will write the results for this node to `<run>/<node_index>.toml`.
    ///
    /// If the folder `<run>/` does not exist, it will be created and the config for that run will be stored in `<run>/config.toml`
    ///
    /// # Panics
    ///
    /// Will panic if serialization to TOML fails
    pub async fn add_result(&mut self, result: RunResults) -> std::io::Result<()>
    where
        K: serde::Serialize,
        E: serde::Serialize,
    {
        let run = result.run.0;
        let folder = run.to_string();
        let folder = Path::new(&folder);
        if !folder.exists() {
            // folder does not exist, create it and copy over the network config to `config.json`
            // we'd use toml here but VRFPubKey cannot be serialized with toml
            let config = &self.configs[run];
            fs::create_dir_all(folder)?;
            fs::write(
                format!("{}/config.json", run),
                serde_json::to_string_pretty(config).expect("Could not serialize"),
            )?;
        }
        fs::write(
            format!("{}/{}.toml", run, result.node_index),
            toml::to_string_pretty(&result).expect("Could not serialize"),
        )?;

        Ok(())
    }

    pub async fn get_next_config(
        &mut self,
        addr: IpAddr,
        sender: OneShotSender<ClientConfig<K, E>>,
        start_round_sender: Sender<ToBackground<K, E>>,
    ) where
        K: Debug + Clone + Send + 'static,
        E: Debug + Clone + Send + 'static,
    {
        let total_runs = self.configs.len();
        let mut config: &mut NetworkConfig<K, E> = match self.configs.get_mut(self.current_run) {
            Some(config) => config,
            None => {
                sender.send(ClientConfig::default());
                return;
            }
        };

        if let Some(libp2p_config) = &mut config.libp2p_config {
            // we are a libp2p orchestrator
            // check to see if we're a bootstrap node
            if self.next_node_index < config.config.num_bootstrap {
                // we're a bootstrap node, add our address and sender to the libp2p_config_sender queue
                self.next_node_index += 1;
                self.libp2p_config_sender.push((addr, sender));
                error!(
                    "Bootstrap nodes {}/{}",
                    self.next_node_index, config.config.num_bootstrap
                );
                if self.next_node_index == config.config.num_bootstrap {
                    // we have enough bootstrap nodes
                    // fill the bootstrap nodes list in `libp2p_config`, then send this to the other nodes
                    libp2p_config.bootstrap_nodes = Vec::new();
                    for (idx, (addr, _sender)) in self.libp2p_config_sender.iter().enumerate() {
                        let pair = libp2p_core::identity::Keypair::generate_ed25519();
                        let port = libp2p_config.base_port + idx as u16;
                        let peer_id = PeerId::from_public_key(&pair.public());
                        error!(" - {peer_id} at {addr}:{port}");
                        libp2p_config.bootstrap_nodes.push((
                            SocketAddr::new(*addr, port),
                            pair.to_protobuf_encoding().unwrap(),
                        ));
                    }
                    for (idx, (addr, sender)) in self.libp2p_config_sender.drain(..).enumerate() {
                        let config =
                            set_config(config.clone(), addr, Run(self.current_run), idx as u64);

                        sender.send(ClientConfig {
                            run: Run(self.current_run),
                            config,
                        });
                    }
                }
                // if we're a bootstrap node, we'll never have to do the run-rollover-logic below.
                // instead we're using the `libp2p_config_sender` queue.
                // so early return here
                return;
            }
        }

        if self.next_node_index >= config.config.total_nodes.get() {
            self.next_node_index = 0;
            self.current_run += 1;

            println!(
                "Starting run {} / {}",
                self.current_run + 1,
                self.configs.len()
            );

            config = match self.configs.get_mut(self.current_run) {
                Some(config) => config,
                None => {
                    sender.send(ClientConfig::default());
                    return;
                }
            };
        } else if self.next_node_index == 0 && self.current_run == 0 {
            println!("Starting run 1 / {}", total_runs);
        }

        let total_nodes = config.config.total_nodes;
        let start_delay_seconds = config.start_delay_seconds;
        let config = set_config(
            config.clone(),
            addr,
            Run(self.current_run),
            self.next_node_index as u64,
        );
        sender.send(ClientConfig {
            run: Run(self.current_run),
            config,
        });

        self.next_node_index += 1;

        if self.next_node_index == total_nodes.get() {
            let run = Run(self.current_run);
            async_spawn(async move {
                tracing::error!(
                    "Reached enough nodes, starting in {} seconds",
                    start_delay_seconds
                );
                async_sleep(Duration::from_secs(start_delay_seconds)).await;
                start_round_sender
                    .send(ToBackground::StartRun(run))
                    .await
                    .expect("Could not start round");
            });
        }
    }

    pub fn current_run_full(&self) -> bool {
        if let Some(config) = self.configs.get(self.current_run) {
            println!(
                "  clients connected: {} / {}",
                self.next_node_index,
                config.config.total_nodes.get()
            );
            self.next_node_index == config.config.total_nodes.get()
        } else {
            false
        }
    }
}

fn set_config<K, E>(
    mut config: NetworkConfig<K, E>,
    public_ip: IpAddr,
    run: Run,
    node_index: u64,
) -> NetworkConfig<K, E> {
    config.node_index = node_index;
    if let Some(libp2p) = &mut config.libp2p_config {
        libp2p.run = run;
        libp2p.node_index = node_index;
        libp2p.public_ip = public_ip;
    }
    config
}
