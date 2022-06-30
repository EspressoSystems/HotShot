use color_eyre::eyre::Result;

use tracing::instrument;

pub mod common;

#[cfg(all(feature = "lossy_network", target_os = "linux"))]
use common::{
    lossy_network::{IsolationConfig, LossyNetworkBuilder},
    ExecutionEnvironment,
};




#[async_std::main]
#[instrument]
async fn main() -> Result<()> {

    #[cfg(all(feature = "lossy_network", target_os = "linux"))]
    {
use rtnetlink::NetemQdisc;
        let network = {
            let mut builder = LossyNetworkBuilder::default();
            builder
            .eth_name("enp1s0".to_string())
            .isolation_config(Some(IsolationConfig::default()))
            .netem_config(NetemQdisc::default())
            .env_type(ExecutionEnvironment::Metal);
            builder.build()?
        };
        network.isolate().await?;
    }

    Ok(())
}
