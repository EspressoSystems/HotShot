use server::{Config, Server};

use async_compatibility_layer::logging::{setup_backtrace, setup_logging};
use clap::Parser;

#[derive(Parser, Debug)]
struct PushCdnArgs {
    url: String,
}

#[cfg_attr(async_executor_impl = "tokio", tokio::main)]
#[cfg_attr(async_executor_impl = "async-std", async_std::main)]
async fn main() {
    setup_backtrace();
    setup_logging();

    // parse comand line args
    let PushCdnArgs { url } = PushCdnArgs::parse();

    // create the server
    let server = Server::new(Config {
        bind_address: url.clone(),
        public_advertise_address: url.clone(),
        private_advertise_address: url,
        redis_url: "redis://127.0.0.1:6379".to_string(),
        redis_password: "".to_string(),
        cert_path: None,
        key_path: None,
    })
    .unwrap();

    // run the server
    server.run().await.unwrap();
}
