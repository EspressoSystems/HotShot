use std::sync::Arc;

use async_compatibility_layer::{art::async_spawn, channel::oneshot};

#[async_std::main]
async fn main() {

    // copied from centralized_web_server trait. 
    let (server_shutdown_sender, server_shutdown) = oneshot();
    let sender = Arc::new(server_shutdown_sender);
    hotshot_centralized_web_server::run_web_server(Some(
        server_shutdown,
    )).await;
}
