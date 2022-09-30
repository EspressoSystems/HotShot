use clap::Parser;
use hotshot_centralized_server::{
    TcpStreamRecvUtil, TcpStreamSendUtil, TcpStreamUtilWithRecv, TcpStreamUtilWithSend,
};
use hotshot_types::traits::signature_key::{
    ed25519::{Ed25519Priv, Ed25519Pub},
    SignatureKey,
};
use hotshot_utils::{
    art::{async_main, split_stream, AsyncReadExt, AsyncWriteExt, TcpStream},
    test_util::setup_logging,
};
use std::{net::ToSocketAddrs, time::Instant};
use tracing::{info, warn};

type ToServer = hotshot_centralized_server::ToServer<Ed25519Pub>;
type FromServer = hotshot_centralized_server::FromServer<Ed25519Pub>;

#[async_main]
async fn main() {
    let opts: Opts = Opts::parse();
    setup_logging();
    let addr = opts
        .addr
        .to_socket_addrs()
        .unwrap_or_else(|e| panic!("Could not resolve addr {}: {e:?}", opts.addr))
        .collect::<Vec<_>>();
    if addr.len() != 1 {
        panic!(
            "{} resolves to {} addresses, cannot continue",
            opts.addr,
            addr.len()
        );
    }
    let (read, write) = split_stream(
        TcpStream::connect(addr[0])
            .await
            .expect("Could not connect to server"),
    );
    let mut write = TcpStreamSendUtil::new(write);
    let mut read = TcpStreamRecvUtil::new(read);
    write.send(ToServer::GetConfig).await.unwrap();
    let config = loop {
        match read.recv::<FromServer>().await.unwrap() {
            FromServer::Config { config, .. } => break config,
            x => warn!("Expected config, got {x:?}"),
        }
    };
    info!("Received config {config:?}");
    let privkey = Ed25519Priv::generated_from_seed_indexed([0u8; 32], config.node_index);
    let pubkey = Ed25519Pub::from_private(&privkey);
    write
        .send(ToServer::Identify { key: pubkey })
        .await
        .unwrap();

    warn!("Waiting for server to start");
    loop {
        match read.recv::<FromServer>().await.unwrap() {
            FromServer::Start => break,
            x => info!("{x:?}"),
        }
    }

    if config.node_index == 0 {
        spam(config.rounds, config.padding, write).await;
    } else {
        read_spam(config.rounds, config.padding, read).await;
    }
}

#[derive(Parser)]
struct Opts {
    pub addr: String,
}

async fn spam(runs: usize, size: usize, mut writer: TcpStreamSendUtil) {
    let mut data = Vec::new();
    data.resize(size, 0);
    let start = Instant::now();
    warn!("Sending {runs} blocks of {size} bytes");
    for i in 0..runs {
        warn!("{i}/{runs}");
        let bytes = (i as u32).to_le_bytes();
        data[..4].copy_from_slice(&bytes);

        writer
            .send(ToServer::Broadcast {
                message_len: data.len() as _,
            })
            .await
            .unwrap();
        writer.write_stream().write_all(&data).await.unwrap();
    }
    let elapsed = start.elapsed();
    let byte_count = size * runs;
    warn!(
        "Sender took {:?}, bytes: {}, mb/s: {}",
        elapsed,
        byte_count,
        byte_count as f64 / elapsed.as_secs_f64() / 1024.0 / 1024.0
    );
}

async fn read_spam(runs: usize, size: usize, mut reader: TcpStreamRecvUtil) {
    let start = Instant::now();
    warn!("Receiving {runs} blocks of {size} bytes");
    let mut expected_run = 0;
    let mut bytes = Vec::new();
    bytes.resize(size, 0);
    loop {
        match reader.recv::<FromServer>().await.unwrap() {
            FromServer::Broadcast { message_len, .. } => {
                let mut idx = 0;
                loop {
                    match reader.recv::<FromServer>().await.unwrap() {
                        FromServer::BroadcastPayload { payload_len, .. } => {
                            reader
                                .read_stream()
                                .read_exact(&mut bytes[idx..idx + payload_len as usize])
                                .await
                                .unwrap();
                            idx += payload_len as usize;
                            if idx == message_len as usize {
                                break;
                            }
                        }
                        x => warn!("Expected broadcast payload, got {x:?} interleaved"),
                    }
                }

                let incoming_run = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
                if incoming_run != expected_run {
                    warn!("Expected run {expected_run}, got {incoming_run} instead, we might have lost a packet");
                }
                warn!("{incoming_run}/{runs}");
                expected_run = incoming_run + 1;
                if expected_run == runs {
                    warn!("Done!");
                    break;
                }
            }
            x => info!("Dropping incoming message {x:?}"),
        }
    }

    let elapsed = start.elapsed();
    let byte_count = runs * size;
    warn!(
        "Reader took {:?}, bytes: {}, mb/s: {}",
        elapsed,
        byte_count,
        byte_count as f64 / elapsed.as_secs_f64() / 1024.0 / 1024.0
    );
}
