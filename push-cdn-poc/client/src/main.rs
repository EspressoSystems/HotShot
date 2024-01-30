use std::{sync::Arc, time::Duration};

use anyhow::Result;
use clap::Parser;
use client::{Client, Config};
use common::{crypto, rng::DeterministicRng, schema::Message, topic::Topic};
use jf_primitives::signatures::{
    bls_over_bn254::BLSOverBN254CurveSignatureScheme as BLS, SignatureScheme,
};
use tokio::{spawn, time::sleep};
use tracing::info;

#[derive(Parser)]
struct Args {
    /// The address to bind to
    #[arg(
        short,
        long,
        default_value = "127.0.0.1:8080",
        // use_value_delimiter = true,
        env = "BROKER_ADDRESS"
    )]
    broker_address: String,

    /// TODO: REMOVE
    #[arg(short, long, env = "INDEX")]
    index: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    // initialize tracing
    tracing_subscriber::fmt::init();

    // parse comand line args
    let Args {
        broker_address,
        index,
    } = Args::parse();

    // create signing and verification key
    let mut prng = DeterministicRng(index);

    let (signing_key, verify_key) = BLS::key_gen(&(), &mut prng).unwrap();

    // create the client
    let client = Client::new(Config {
        broker_address,
        signing_key,
        initial_topics: Vec::from([Topic::Global]),
        verify_key,
    })
    .unwrap();

    let client_ = client.clone();
    client_.wait_connect().await;
    let f = spawn(async move {
        sleep(Duration::from_secs(1)).await;
        // send message
        let data = vec![index as u8];
        let (_, recipient) = {
            if index == 0 {
                BLS::key_gen(&(), &mut DeterministicRng(1)).unwrap()
            } else {
                BLS::key_gen(&(), &mut DeterministicRng(0)).unwrap()
            }
        };

        let recipient_bytes = crypto::serialize(&recipient).unwrap();

        let _ = client_
            .send_message(Arc::new(Message::Direct(recipient_bytes, data.clone())))
            .await;
    });

    let message = client.recv_message().await;
    info!("{:?}", message);

    let _ = f.await;

    Ok(())
}
