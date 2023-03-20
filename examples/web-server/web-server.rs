use std::sync::Arc;

use async_compatibility_layer::channel::oneshot;

#[async_std::main]
async fn main() {
    let (server_shutdown_sender, server_shutdown) = oneshot();
    let _sender = Arc::new(server_shutdown_sender);
    let _result = hotshot_web_server::run_web_server(Some(server_shutdown)).await;
}
