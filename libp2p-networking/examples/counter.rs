use std::{panic::PanicInfo, time::Duration};

use async_std::{
    future::timeout,
    task::{spawn, JoinHandle},
};
use clap::Parser;
use color_eyre::eyre::Result;

pub mod common;

#[cfg(all(feature = "lossy_network", target_os = "linux"))]
use common::{
    lossy_network::{IsolationConfig, LossyNetworkBuilder},
    ExecutionEnvironment,
};

use common::{start_main, CliOpt};
use futures::future::join_all;
use tracing::instrument;

#[async_std::main]
#[instrument]
async fn main() -> Result<()> {
    Ok(())
}
