use async_std::process::Command;
use color_eyre::eyre::Result;
use structopt::StructOpt;
use tracing::info;
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
        let mut builder = LossyNetworkBuilder::default();
        builder.env_type(args.env_type);
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

    // check is correct
    let cmdout = Command::new("ip").arg("a").output().await?.stdout;
    info!("OUTPUT: {}", std::str::from_utf8(&cmdout).unwrap());

    // ping google for sanity check
    let cmdout = Command::new("ping")
        .args(["-c", "2", "142.250.80.68"])
        .output()
        .await?;
    info!(
        "OUTPUT: {} {}",
        std::str::from_utf8(&cmdout.stdout).unwrap(),
        std::str::from_utf8(&cmdout.stderr).unwrap()
    );

    // list qdisc interfaces to ensure netem exists
    let cmdout = Command::new("tc").args(["qdisc", "list"]).output().await?;
    info!(
        "OUTPUT: {} {}",
        std::str::from_utf8(&cmdout.stdout).unwrap(),
        std::str::from_utf8(&cmdout.stderr).unwrap()
    );

    start_main(args).await?;

    #[cfg(feature = "lossy_network")]
    {
        // implicitly deletes qdisc in the case of metal run
        // leaves qdisc alive in docker run with expectation docker does cleanup
        network.undo_isolate().await?;
    }

    Ok(())
}
