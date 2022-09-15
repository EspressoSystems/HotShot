use crate::{ClientConfig, NetworkConfig, Run, RunResults, ToBackground};
use std::{fs, path::Path};
use flume::Sender;
use libp2p_core::PeerId;
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tracing::error;
use hotshot_utils::art::{async_spawn, async_sleep};

/// Contains information about the current round
pub struct RoundConfig<K> {
    configs: Vec<NetworkConfig<K>>,
    libp2p_config_sender: Vec<(IpAddr, Sender<ClientConfig<K>>)>,
    current_run: usize,
    next_node_index: usize,
}

impl<K> RoundConfig<K> {
    pub fn new(configs: Vec<NetworkConfig<K>>) -> Self {
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
    {
        let run = result.run.0;
        let folder = run.to_string();
        let folder = Path::new(&folder);
        if !folder.exists() {
            // folder does not exist, create it and copy over the network config to `config.toml`
            let config = &self.configs[run];
            fs::create_dir_all(folder)?;
            fs::write(
                format!("{}/config.toml", run),
                toml::to_string_pretty(config).expect("Could not serialize"),
            )
            ?;
        }
        fs::write(
            format!("{}/{}.toml", run, result.node_index),
            toml::to_string_pretty(&result).expect("Could not serialize"),
        )
        ?;

        Ok(())
    }

    pub async fn get_next_config(
        &mut self,
        addr: IpAddr,
        sender: Sender<ClientConfig<K>>,
        start_round_sender: Sender<ToBackground<K>>,
    ) where
        K: Clone + Send + 'static,
    {
        let total_runs = self.configs.len();
        let mut config: &mut NetworkConfig<K> = match self.configs.get_mut(self.current_run) {
            Some(config) => config,
            None => {
                let _ = sender.send_async(ClientConfig::default()).await;
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
                    for (idx, (addr, sender)) in self.libp2p_config_sender.iter().enumerate() {
                        let config =
                            set_config(config.clone(), *addr, Run(self.current_run), idx as u64);

                        let _ = sender.send(ClientConfig {
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
                    let _ = sender.send_async(ClientConfig::default()).await;
                    return;
                }
            };
        } else if self.next_node_index == 0 && self.current_run == 0 {
            println!("Starting run 1 / {}", total_runs);
        }

        let total_nodes = config.config.total_nodes;
        let config = set_config(
            config.clone(),
            addr,
            Run(self.current_run),
            self.next_node_index as u64,
        );
        let _ = sender
            .send_async(ClientConfig {
                run: Run(self.current_run),
                config,
            })
            .await;

        self.next_node_index += 1;

        if self.next_node_index == total_nodes.get() {
            let run = Run(self.current_run);
            async_spawn(async move {
                tracing::error!("Reached enough nodes, starting in 60 seconds");
                async_sleep(Duration::from_secs(60)).await;
                start_round_sender
                    .send_async(ToBackground::StartRun(run))
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

fn set_config<K>(
    mut config: NetworkConfig<K>,
    public_ip: IpAddr,
    run: Run,
    node_index: u64,
) -> NetworkConfig<K>
where
    K: Clone,
{
    config.node_index = node_index;
    if let Some(libp2p) = &mut config.libp2p_config {
        libp2p.run = run;
        libp2p.node_index = node_index;
        libp2p.public_ip = public_ip;
    }
    config
}
