use async_std::fs::File;
use async_std::os::unix::io::IntoRawFd;
use async_std::process::Command;
use color_eyre::eyre::Result;
use common::ExecutionEnvironment;
use common::{start_main, CliOpt};
use structopt::StructOpt;
use tracing::info;
use tracing::instrument;

use crate::common::lossy_network::IsolationConfig;
pub mod common;

#[async_std::main]
#[instrument]
async fn main() -> Result<()> {
    let args = CliOpt::from_args();

    #[cfg(feature = "lossy_network")]
    {
        let network = {
            let builder = LossyNetworkBuilder::default().env_type(args.env_type);
            match args.env_type {
                ExecutionEnvironment::Docker => builder.eth_name("eth0").isolation_config(None),
                ExecutionEnvironment::Metal => builder
                    .eth_name("ens5")
                    .isolation_config(Some(IsolationConfig::default())),
            }
            builder.build()
        };
        network.isolate()?;

        network.create_qdisc(network.eth)?;
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

    let args = CliOpt::from_args();
    start_main(args).await?;

    #[cfg(feature = "lossy_network")]
    {
        // implicitly deletes qdisc in the case of metal run
        // leaves qdisc alive in docker run with expectation docker does cleanup
        network.undo_isolate()?;
    }

    Ok(())
}
