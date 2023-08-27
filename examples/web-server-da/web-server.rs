use hotshot::demos::sdemo::SDemoTypes;
use std::sync::Arc;

use async_compatibility_layer::{
    channel::oneshot,
    logging::{setup_backtrace, setup_logging},
};
use clap::Parser;

#[derive(Parser, Debug)]
struct WebServerArgs {
    port: u16,
}
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::main(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::main)]
async fn main() {
    setup_backtrace();
    setup_logging();
    let args = WebServerArgs::parse();
    let (server_shutdown_sender, server_shutdown) = oneshot();
    let _sender = Arc::new(server_shutdown_sender);
    let _result = hotshot_web_server::run_web_server::<
        <SDemoTypes as hotshot_types::traits::node_implementation::NodeType>::SignatureKey,
    >(Some(server_shutdown), args.port)
    .await;
}
