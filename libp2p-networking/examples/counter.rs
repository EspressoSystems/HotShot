use color_eyre::eyre::Result;
use structopt::StructOpt;
use tracing::instrument;

pub mod common;

#[cfg(feature = "lossy_network")]
use common::{
    lossy_network::{IsolationConfig, LossyNetworkBuilder},
    ExecutionEnvironment,
};

use common::{start_main, CliOpt};

#[async_std::main]
#[instrument]
async fn main() -> Result<()> {
    let args = CliOpt::from_args();

    #[cfg(feature = "lossy_network")]
    let network = {
        use crate::common::lossy_network::LOSSY_QDISC;
        let mut builder = LossyNetworkBuilder::default();
        builder.env_type(args.env_type).netem_config(LOSSY_QDISC);
        match args.env_type {
            ExecutionEnvironment::Docker => {
                builder.eth_name("eth0".to_string()).isolation_config(None)
            }
            ExecutionEnvironment::Metal => builder
                .eth_name("ens5".to_string())
                .isolation_config(Some(IsolationConfig::default())),
        };
        builder.build()
    }?;

    #[cfg(feature = "lossy_network")]
    {
        network.isolate().await?;
        network.create_qdisc().await?;
    }

    start_main(args).await?;

    #[cfg(feature = "lossy_network")]
    {
        // implicitly deletes qdisc in the case of metal run
        // leaves qdisc alive in docker run with expectation docker does cleanup
        network.undo_isolate().await?;
    }

    Ok(())
}
