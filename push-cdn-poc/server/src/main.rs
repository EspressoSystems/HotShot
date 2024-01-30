use anyhow::Result;
use clap::Parser;
use common::{crypto, err, error::BrokerError};
use jf_primitives::signatures::bls_over_bn254::BLSOverBN254CurveSignatureScheme as BLS;
use server::{Config, Server};
use tracing::info;

#[derive(Parser)]
struct Args {
    /// The address to bind to
    #[arg(
        short,
        long,
        default_value = "127.0.0.1:8080",
        env = "BROKER_BIND_ADDRESS"
    )]
    bind_address: String,

    /// The address to advertise to other brokers
    #[arg(
        long,
        default_value = "127.0.0.1:8080",
        env = "BROKER_PUBLIC_ADVERTISE_ADDRESS"
    )]
    public_advertise_address: String,

    /// The address to advertise to other brokers
    #[arg(
        long,
        default_value = "127.0.0.1:8080",
        env = "BROKER_PRIVATE_ADVERTISE_ADDRESS"
    )]
    private_advertise_address: String,

    /// The URL of the Redis instance for peer discovery, including scheme
    #[arg(
        short,
        long,
        default_value = "redis://127.0.0.1:6379",
        env = "BROKER_REDIS_URL"
    )]
    redis_url: String,

    /// The password for the Redis instance
    #[arg(long, default_value = "127.0.0.1:6379", env = "BROKER_REDIS_PASSWORD")]
    redis_password: String,

    /// The path to the certificate file
    /// If neither cert or key paths are specified, the server will generate a self-signed certificate
    #[arg(long, env = "BROKER_TLS_CERT_PATH")]
    tls_cert_path: Option<String>,

    /// The path to the key file
    /// If neither cert or key paths are specified, the server will generate a self-signed certificate
    #[arg(long, env = "BROKER_TLS_KEY_PATH")]
    tls_key_path: Option<String>,

    /// The broker-specific key used to authenticate brokers with each other. If not provided, a random key will be generated.
    #[arg(
        long,
        env = "BROKER_SIGNING_KEY_HEX"
    )]
    signing_key_hex: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // initialize tracing
    tracing_subscriber::fmt::init();

    // parse comand line args
    let Args {
        bind_address,
        public_advertise_address,
        private_advertise_address,
        redis_url,
        redis_password,
        tls_cert_path,
        tls_key_path,
        signing_key_hex,
    } = Args::parse();

    // parse signing key or pass through None
    let signing_key =
        signing_key_hex.map(|hex| crypto::deserialize(hex::decode(hex).unwrap()).unwrap());

    // create the server
    let server = Server::new(Config {
        bind_address: bind_address.clone(),
        public_advertise_address,
        private_advertise_address,
        redis_url,
        redis_password,
        tls_cert_path,
        tls_key_path,
        signing_key,
    })?;

    info!("starting broker on {}", bind_address);

    // run the server
    Ok(server.run().await?)
}
