use async_std::fs::File;
use async_std::process::Command;
use nix::sched::setns;
use nix::sched::CloneFlags;
use std::io::Result;
use tracing::info;

use super::ExecutionEnvironment;
use std::os::unix::io::IntoRawFd;

/// A description of a lossy network
#[derive(Clone, derive_builder::Builder, custom_debug::Debug)]
pub struct LossyNetwork {
    /// ethernet interface that is connected to WAN
    eth_name: String,
    /// metadata describing how to isolate. Only used when [`env_type`] is [`Metal`]
    isolation_config: Option<IsolationConfig>,
    /// The network loss conditions
    #[builder(default)]
    netem_config: NetemConfig,
    /// the execution environment
    env_type: ExecutionEnvironment,
}

impl LossyNetwork {
    // Create isolated environment in separate network namespace via network bridge
    pub async fn isolate(&self) -> Result<()> {
        if let Some(ref isolation_config) = self.isolation_config {
            isolation_config.isolate(&self.eth_name).await?
        }
        Ok(())
    }

    // delete isolated environment and network bridge
    pub async fn undo_isolate(&self) -> Result<()> {
        if let Some(ref isolation_config) = self.isolation_config {
            isolation_config.undo_isolate(self.eth_name.clone()).await?
        }
        Ok(())
    }

    // create a network qdisc
    pub async fn create_qdisc(&self) -> Result<()> {
        match self.env_type {
            ExecutionEnvironment::Docker => {
                self.netem_config.create_qdisc(&self.eth_name).await?;
            }
            ExecutionEnvironment::Metal => {
                self.netem_config
                    .create_qdisc(&self.isolation_config.as_ref().unwrap().veth2_name.clone())
                    .await?;
            }
        }
        Ok(())
    }
}

/// A description of how the network should be isolated
#[derive(Clone, derive_builder::Builder, custom_debug::Debug)]
#[builder(default)]
pub struct IsolationConfig {
    /// the network namespace name to create
    counter_ns: String,
    /// the bridge ip address
    bridge_addr: String,
    /// the bridge name
    bridge_name: String,
    /// the virtual ethernet interface name
    /// that lives in the default/root network namespace
    veth_name: String,
    /// the virtual ethernet interface name
    /// that lives in [`counter_ns`]
    veth2_name: String,
    /// the virtual ethernet interface ip address
    /// that lives in [`counter_ns`]
    veth2_addr: String,
}

/// hardcoded default values
impl Default for IsolationConfig {
    fn default() -> Self {
        Self {
            counter_ns: "COUNTER_NS".to_string(),
            bridge_addr: "172.18.0.1".to_string(),
            bridge_name: "br0".to_string(),
            veth_name: "veth1".to_string(),
            veth2_name: "veth2".to_string(),
            veth2_addr: "172.18.0.2".to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NetemDelay {
    /// in ms
    base: u32,
    /// in ms
    jitter: Option<u32>,
    /// percentage depends on previous value
    /// (should range from 0-100)
    correlation: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
pub struct NetemLoss {
    /// percentage packets dropped
    base: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct NetemCorruption {
    /// percentage packets with corrupted bytes
    /// (should range from 0-100)
    base: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct NetemReorder {
    /// percentage packets with corrupted bytes
    /// (should range from 0-100)
    base: u32,
    /// percentage depends on previous value
    /// (should range from 0-100)
    correlation: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct NetemConfig {
    delay: Option<NetemDelay>,
    loss: Option<NetemLoss>,
    corruption: Option<NetemCorruption>,
    reorder: Option<NetemReorder>,
}

impl Default for NetemConfig {
    fn default() -> Self {
        Self {
            delay: Some(NetemDelay {
                base: 30,
                jitter: Some(10),
                correlation: Some(20),
            }),
            loss: Some(NetemLoss { base: 10 }),
            corruption: Some(NetemCorruption { base: 5 }),
            reorder: Some(NetemReorder {
                base: 5,
                correlation: Some(5),
            }),
        }
    }
}

impl NetemConfig {
    async fn create_qdisc(&self, veth: &str) -> Result<()> {
        let mut args = ["qdisc", "add", "dev", veth, "netem"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        if let Some(delay) = self.delay {
            args.push(format!("{}ms", delay.base));
            if let Some(jitter) = delay.jitter {
                args.push(format!("{}ms", jitter));
            }
            if let Some(correlation) = delay.correlation {
                args.push(format!("{}%", correlation));
            }
        }

        if let Some(loss) = self.loss {
            args.push(format!("{}%", loss.base));
        }

        if let Some(corruption) = self.corruption {
            args.push(format!("{}%", corruption.base));
        }

        if let Some(reorder) = self.reorder {
            args.push(format!("{}%", reorder.base));
            if let Some(correlation) = reorder.correlation {
                args.push(format!("{}%", correlation));
            }
        }

        info!("{:?}", Command::new("tc").args(args).output().await?.status);
        Ok(())
    }
}

impl IsolationConfig {
    /// Prepares server for latency by:
    /// - creating a separate network namespace denoted [`counter_ns`]
    /// - creating a virtual ethernet device (veth2) in this namespace
    /// - bridging the virtual ethernet device within COUNTER_NS to the default/root network namespace
    /// - adding firewall rules to allow traffic to flow between the network bridge and outside world
    /// - execute the demo inside network namespace
    async fn isolate(&self, eth_name: &str) -> Result<()> {
        // generate isolated network namespace
        info!(
            "{:?}",
            Command::new("ip")
                .args(["netns", "add", &self.counter_ns])
                .output()
                .await?
                .status
        );

        // create veth link between veth and veth2
        // ip link add $veth_name type veth peer name $veth2_name
        info!(
            "{:?}",
            Command::new("ip")
                .args([
                    "link",
                    "add",
                    &self.veth_name,
                    "type",
                    "veth",
                    "peer",
                    "name",
                    &self.veth2_name
                ])
                .output()
                .await?
                .status
        );

        // setup veth link
        // ip link set $veth_name up
        info!(
            "{:?}",
            Command::new("ip")
                .args(["link", "set", &self.veth_name, "up"])
                .output()
                .await?
                .status
        );

        // move veth2 to coutner namespace
        // ip link set $veth2_name netns $counter_ns
        info!(
            "{:?}, ",
            Command::new("ip")
                .args(["link", "set", &self.veth2_name, "netns", &self.counter_ns])
                .output()
                .await?
                .status
        );

        // activate loopback interface in counter ns
        // ip netns exec $counter_ns ip link set lo up
        info!(
            "{:?}",
            Command::new("ip")
                .args([
                    "netns",
                    "exec",
                    &self.counter_ns,
                    "ip",
                    "link",
                    "set",
                    "lo",
                    "up"
                ])
                .output()
                .await?
                .status
        );

        // bring veth2 up
        // ip netns exec $counter_ns ip link set $veth2_name up
        info!(
            "{:?}",
            Command::new("ip")
                .args([
                    "netns",
                    "exec",
                    &self.counter_ns,
                    "ip",
                    "link",
                    "set",
                    &self.veth2_name,
                    "up"
                ])
                .output()
                .await?
                .status
        );

        // assign ip address to veth2
        // ip netns exec $counter_ns ip addr add $veth2_addr/16 dev $veth2_name
        info!(
            "{:?}",
            Command::new("ip")
                .args([
                    "netns",
                    "exec",
                    &self.counter_ns,
                    "ip",
                    "addr",
                    "add",
                    &format!("{}/16", self.veth2_addr),
                    "dev",
                    &self.veth2_name
                ])
                .output()
                .await?
                .status
        );

        // create bridge and bring up bridge
        // ip link add $bridge_name type bridge
        info!(
            "{:?}",
            Command::new("ip")
                .args(["link", "add", &self.bridge_name, "type", "bridge"])
                .output()
                .await?
                .status
        );
        // ip link set $bridge_name up
        info!(
            "{:?}",
            Command::new("ip")
                .args(["link", "set", &self.bridge_name, "up"])
                .output()
                .await?
                .status
        );

        // assign veth to bridge
        // ip link set $veth_name master $bridge_name
        info!(
            "{:?}",
            Command::new("ip")
                .args(["link", "set", &self.veth_name, "master", &self.bridge_name])
                .output()
                .await?
                .status
        );

        // setup bridge ip
        // ip addr add $bridge_addr/16 dev $bridge_name
        info!(
            "{:?}",
            Command::new("ip")
                .args([
                    "addr",
                    "add",
                    &format!("{}/16", self.bridge_addr),
                    "dev",
                    &self.bridge_name
                ])
                .output()
                .await?
                .status
        );

        // add default routes for ns
        // ip netns exec $counter_ns ip route add default via $bridge_addr
        info!(
            "{:?}",
            Command::new("ip")
                .args([
                    "netns",
                    "exec",
                    &self.counter_ns,
                    "ip",
                    "route",
                    "add",
                    "default",
                    "via",
                    &self.bridge_addr
                ])
                .output()
                .await?
                .status
        );

        // accept traffic on bridge
        // iptables -A FORWARD -o ens5 -i br0 -j ACCEPT
        info!(
            "{:?}",
            Command::new("iptables")
                .args(["-A", "FORWARD", "-o", eth_name, "-i", "br0", "-j", "ACCEPT"])
                .output()
                .await?
                .status
        );
        // iptables -A FORWARD -i ens5 -o br0 -j ACCEPT
        info!(
            "{:?}",
            Command::new("iptables")
                .args(["-A", "FORWARD", "-i", eth_name, "-o", "br0", "-j", "ACCEPT"])
                .output()
                .await?
                .status
        );
        // NAT
        // iptables -t nat -A POSTROUTING -s 172.20.0.1 -o ens5 -j MASQUERADE
        info!(
            "{:?}",
            Command::new("iptables")
                .args([
                    "-t",
                    "nat",
                    "-A",
                    "POSTROUTING",
                    "-s",
                    &self.bridge_addr,
                    "-o",
                    "ens5",
                    "-j",
                    "MASQUERADE"
                ])
                .output()
                .await?
                .status
        );
        // iptables -t nat -A POSTROUTING -o br0 -j MASQUERADE
        info!(
            "{:?}",
            Command::new("iptables")
                .args([
                    "-t",
                    "nat",
                    "-A",
                    "POSTROUTING",
                    "-o",
                    &self.bridge_name,
                    "-j",
                    "MASQUERADE"
                ])
                .output()
                .await?
                .status
        );
        // iptables -t nat -A POSTROUTING -s 172.20.0.0/16 -j MASQUERADE
        info!(
            "{:?}",
            Command::new("iptables")
                .args([
                    "-t",
                    "nat",
                    "-A",
                    "POSTROUTING",
                    "-s",
                    &format!("{}/16", &self.bridge_addr),
                    "-j",
                    "MASQUERADE"
                ])
                .output()
                .await?
                .status
        );

        // switch into network namespace network
        let counter_ns_fd = File::open(format!("/var/run/netns/{}", self.counter_ns))
            .await?
            .into_raw_fd();
        setns(counter_ns_fd, CloneFlags::CLONE_NEWNET)?;

        // add example netem qdisc
        // TODO switch this into something parametrized
        // tc qdisc add dev enp5s0 root netem delay 100ms 10ms 25% distribution normal loss 0.3% 25% corrupt 0.1% reorder 25% 50%
        info!(
            "{:?}",
            Command::new("tc")
                .args([
                    "qdisc",
                    "add",
                    "dev",
                    &self.veth2_name,
                    "root",
                    "netem",
                    "delay",
                    "20ms",
                    "1ms",
                    "25%",
                    "distribution",
                    "normal",
                    "loss",
                    "0.3%",
                    "corrupt",
                    "0.1%",
                    "reorder",
                    "1%",
                    "2%"
                ])
                .output()
                .await?
                .status
        );
        Ok(())
    }

    /// tears down all created interfaces
    /// deletes all iptables rules
    /// deletes namespace
    async fn undo_isolate(&self, eth_name: String) -> Result<()> {
        let root_ns_fd = File::open("/proc/1/ns/net").await?.into_raw_fd();
        setns(root_ns_fd, CloneFlags::CLONE_NEWNET)?;

        // delete namespace
        Command::new("ip")
            .args(["netns", "delete", &self.counter_ns])
            .output()
            .await?;
        // delete remaining bridge
        Command::new("ip")
            .args(["link", "delete", &self.bridge_name])
            .output()
            .await?;
        // delete creates iptables rules
        // iptables -D FORWARD -o ens5 -i br0 -j ACCEPT
        info!(
            "{:?}",
            Command::new("iptables")
                .args(["-D", "FORWARD", "-o", &eth_name, "-i", "br0", "-j", "ACCEPT"])
                .output()
                .await?
                .status
        );
        // iptables -D FORWARD -i ens5 -o br0 -j ACCEPT
        info!(
            "{:?}",
            Command::new("iptables")
                .args(["-D", "FORWARD", "-i", &eth_name, "-o", "br0", "-j", "ACCEPT"])
                .output()
                .await?
                .status
        );
        // iptables -t nat -D POSTROUTING -s $bridge_addr -o ens5 -j MASQUERADE
        info!(
            "{:?}",
            Command::new("iptables")
                .args([
                    "-t",
                    "nat",
                    "-D",
                    "POSTROUTING",
                    "-s",
                    &self.bridge_addr,
                    "-o",
                    "ens5",
                    "-j",
                    "MASQUERADE"
                ])
                .output()
                .await?
                .status
        );
        // iptables -t nat -D POSTROUTING -o br0 -j MASQUERADE
        info!(
            "{:?}",
            Command::new("iptables")
                .args([
                    "-t",
                    "nat",
                    "-D",
                    "POSTROUTING",
                    "-o",
                    &self.bridge_name,
                    "-j",
                    "MASQUERADE"
                ])
                .output()
                .await?
                .status
        );
        // iptables -t nat -D POSTROUTING -s $bridge_addr/16 -j MASQUERADE
        info!(
            "{:?}",
            Command::new("iptables")
                .args([
                    "-t",
                    "nat",
                    "-D",
                    "POSTROUTING",
                    "-s",
                    &format!("{}/16", self.bridge_addr),
                    "-j",
                    "MASQUERADE"
                ])
                .output()
                .await?
                .status
        );
        Ok(())
    }
}
