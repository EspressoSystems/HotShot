use hotshot::demos::vdemo::VDemoTypes;
use hotshot_web_server::config::DEFAULT_WEB_SERVER_DA_PORT;
use std::sync::Arc;

use async_compatibility_layer::channel::oneshot;

#[async_std::main]
async fn main() {
    let (server_shutdown_sender, server_shutdown) = oneshot();
    let _sender = Arc::new(server_shutdown_sender);
    let _result = hotshot_web_server::run_web_server::<
        <VDemoTypes as hotshot_types::traits::node_implementation::NodeType>::SignatureKey,
    >(Some(server_shutdown), DEFAULT_WEB_SERVER_DA_PORT)
    .await;
}
